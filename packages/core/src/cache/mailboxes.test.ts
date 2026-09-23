// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { JmapClient, MethodFailure } from "../jmap/client";
import { ACCOUNT, FakeJmap, json, mailbox } from "./fake-jmap";
import { syncMailboxes } from "./mailboxes";
import { MemoryMailStore } from "./memory";

function serve(): { server: FakeJmap; client: JmapClient; store: MemoryMailStore } {
  const server = new FakeJmap();
  server.putMailbox(mailbox("inbox", "inbox"));
  server.putMailbox(mailbox("sent", "sent"));
  vi.stubGlobal("fetch", server.fetch);
  return { server, client: new JmapClient(ACCOUNT), store: new MemoryMailStore() };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("the first sync fetches every mailbox in one call and keeps the state", async () => {
  const { server, client, store } = serve();
  expect(await syncMailboxes(client, store)).toBe(true);
  expect(server.posted()).toEqual([[["Mailbox/get", { accountId: "u1", ids: null }, "m"]]]);
  const rows = await store.mailboxes(ACCOUNT);
  expect(rows.map((row) => row.id)).toEqual(["inbox", "sent"]);
  expect(rows[0]?.syncedEmails).toBe(0);
  expect(await store.state(ACCOUNT, "Mailbox")).toBe(String(server.sequence));
});

test("a later sync asks the changes and the rows they name in one round trip", async () => {
  const { server, client, store } = serve();
  await syncMailboxes(client, store);
  expect(await syncMailboxes(client, store)).toBe(false);
  server.putMailbox({ ...mailbox("inbox", "inbox"), unreadEmails: 4 });
  server.putMailbox(mailbox("drafts", "drafts"));
  server.removeMailbox("sent");
  expect(await syncMailboxes(client, store)).toBe(true);
  const calls = server
    .posted()
    .at(-1)
    ?.map(([name, , id]) => [name, id]);
  expect(calls).toEqual([
    ["Mailbox/changes", "c:Mailbox"],
    ["Mailbox/get", "n:Mailbox"],
    ["Mailbox/get", "u:Mailbox"],
  ]);
  const rows = new Map((await store.mailboxes(ACCOUNT)).map((row) => [row.id, row]));
  expect([...rows.keys()].toSorted()).toEqual(["drafts", "inbox"]);
  expect(rows.get("inbox")?.unreadEmails).toBe(4);
  expect(await store.state(ACCOUNT, "Mailbox")).toBe(String(server.sequence));
});

test("a state past the horizon fetches every mailbox anew", async () => {
  const { server, client, store } = serve();
  await syncMailboxes(client, store);
  server.putMailbox(mailbox("drafts", "drafts"));
  server.forget();
  expect(await syncMailboxes(client, store)).toBe(true);
  expect(
    server
      .posted()
      .at(-1)
      ?.map(([name]) => name),
  ).toEqual(["Mailbox/get"]);
  expect((await store.mailboxes(ACCOUNT)).map((row) => row.id)).toEqual([
    "inbox",
    "sent",
    "drafts",
  ]);
});

test("hasMoreChanges is followed round by round", async () => {
  const { server, client, store } = serve();
  await syncMailboxes(client, store);
  server.changesCap = 1;
  server.putMailbox(mailbox("a", null));
  server.putMailbox(mailbox("b", null));
  server.putMailbox(mailbox("c", null));
  expect(await syncMailboxes(client, store)).toBe(true);
  expect(server.posted()).toHaveLength(4);
  expect((await store.mailboxes(ACCOUNT)).map((row) => row.id)).toContain("c");
  expect(await store.state(ACCOUNT, "Mailbox")).toBe(String(server.sequence));
});

test("without the vendor capability the rows carry no synced count", async () => {
  const { server, client, store } = serve();
  server.vendor = false;
  await syncMailboxes(client, store);
  expect((await store.mailboxes(ACCOUNT))[0]?.syncedEmails).toBeUndefined();
});

test("a refusal other than the horizon is the caller's", async () => {
  const { server, client, store } = serve();
  await syncMailboxes(client, store);
  server.queue.push(
    json(200, {
      methodResponses: [["error", { type: "serverFail" }, "c:Mailbox"]],
      sessionState: "s1",
    }),
  );
  await expect(syncMailboxes(client, store)).rejects.toBeInstanceOf(MethodFailure);
});
