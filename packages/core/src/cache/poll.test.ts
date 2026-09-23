// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { JmapClient } from "../jmap/client";
import { ACCOUNT, FakeJmap, at, email, mailbox } from "./fake-jmap";
import { CHANGES_ROUNDS_MAX, WINDOW_SIZE } from "./limits";
import { syncMailboxes } from "./mailboxes";
import { MemoryMailStore } from "./memory";
import { applyChanges } from "./poll";
import { queryWindow, revealNewMail } from "./window";

interface Rig {
  server: FakeJmap;
  client: JmapClient;
  store: MemoryMailStore;
}

// `count` emails in the inbox, one per thread, e1 the oldest; the
// mailboxes and the first page are held before the poll runs.
async function serve(count: number): Promise<Rig> {
  const server = new FakeJmap();
  server.putMailbox(mailbox("inbox", "inbox"));
  server.putMailbox(mailbox("archive", "archive"));
  for (let index = 1; index <= count; index += 1) {
    server.addEmail(email(`e${String(index)}`, { receivedAt: at(index) }));
  }
  vi.stubGlobal("fetch", server.fetch);
  const rig = { server, client: new JmapClient(ACCOUNT), store: new MemoryMailStore() };
  await syncMailboxes(rig.client, rig.store);
  await queryWindow(rig.client, rig.store, "inbox", 0);
  server.requests.length = 0;
  return rig;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("an account without states asks nothing", async () => {
  const server = new FakeJmap();
  vi.stubGlobal("fetch", server.fetch);
  const applied = await applyChanges(new JmapClient(ACCOUNT), new MemoryMailStore(), []);
  expect(applied).toEqual({ mailboxes: false, windows: [], threads: [] });
  expect(server.requests).toHaveLength(0);
});

test("a flag change patches the header row and the thread member in one round trip and moves no list", async () => {
  const { server, client, store } = await serve(5);
  server.amend("e5", { keywords: { $seen: true, $flagged: true } });
  const applied = await applyChanges(client, store, ["inbox"]);
  expect(server.posted()).toHaveLength(1);
  expect(server.posted()[0]?.map(([name, , id]) => `${name}#${id}`)).toEqual([
    "Mailbox/changes#c:Mailbox",
    "Mailbox/get#n:Mailbox",
    "Mailbox/get#u:Mailbox",
    "Email/changes#c:Email",
    "Email/get#n:Email",
    "Email/get#u:Email",
    "Thread/changes#c:Thread",
    "Thread/get#u:Thread",
    "Email/get#m:Thread",
  ]);
  expect(applied).toEqual({ mailboxes: false, windows: [], threads: [] });
  expect((await store.emails(ACCOUNT, ["e5"])).get("e5")?.keywords).toEqual({
    $seen: true,
    $flagged: true,
  });
  const thread = (await store.threads(ACCOUNT, ["t-e5"])).get("t-e5");
  expect(thread?.members["e5"]?.keywords).toEqual({ $seen: true, $flagged: true });
  expect(await store.state(ACCOUNT, "Email")).toBe(String(server.sequence));
});

test("new mail waits behind the marker until the user brings it in", async () => {
  const { server, client, store } = await serve(120);
  await queryWindow(client, store, "inbox", 1);
  server.requests.length = 0;
  server.addEmail(email("e121", { receivedAt: at(121) }));
  server.addEmail(email("e122", { receivedAt: at(122) }));
  const applied = await applyChanges(client, store, ["inbox"]);
  expect(applied.windows).toEqual(["inbox"]);
  expect(server.posted()).toHaveLength(2);
  const held = await queryWindow(client, store, "inbox", 0);
  expect(held.ids[0]).toBe("e120");
  expect(held.ids).toHaveLength(WINDOW_SIZE);
  expect(held.pending).toBe(2);
  expect(held.total).toBe(120);
  expect(held.emails["e120"]).toBeDefined();
  await revealNewMail(store, ACCOUNT, "inbox");
  const revealed = await queryWindow(client, store, "inbox", 0);
  expect(revealed.ids.slice(0, 3)).toEqual(["e122", "e121", "e120"]);
  expect(revealed.pending).toBe(0);
  expect(revealed.total).toBe(122);
  expect(revealed.emails["e122"]?.subject).toBe("Message e122");
  expect((await store.query(ACCOUNT, "inbox"))?.pages.map((page) => page.page)).toEqual([0]);
});

test("a reply to a shown thread updates its members and waits as new mail while the shown row stays", async () => {
  const { server, client, store } = await serve(120);
  server.addEmail(email("e121", { threadId: "t-e50", receivedAt: at(121) }));
  const applied = await applyChanges(client, store, ["inbox"]);
  expect(applied.threads).toEqual(["t-e50"]);
  const page = await queryWindow(client, store, "inbox", 0);
  expect(page.ids).toContain("e50");
  expect(page.pending).toBe(1);
  const thread = page.threads["t-e50"];
  expect(thread?.emailIds).toEqual(["e50", "e121"]);
  expect(Object.keys(thread?.members ?? {})).toEqual(["e50", "e121"]);
  await revealNewMail(store, ACCOUNT, "inbox");
  const revealed = await queryWindow(client, store, "inbox", 0);
  expect(revealed.ids[0]).toBe("e121");
  expect(revealed.ids).not.toContain("e50");
});

test("a destroyed row and one moved out leave the list at once and older mail that follows lands", async () => {
  const { server, client, store } = await serve(120);
  server.destroyEmail("e120");
  server.amend("e119", { mailboxIds: { archive: true } });
  const applied = await applyChanges(client, store, ["inbox"]);
  expect(applied.windows).toEqual(["inbox"]);
  const page = await queryWindow(client, store, "inbox", 0);
  expect(page.ids).not.toContain("e120");
  expect(page.ids).not.toContain("e119");
  expect(page.ids).toHaveLength(WINDOW_SIZE);
  expect(page.ids.at(-1)).toBe("e19");
  expect(page.pending).toBe(0);
  expect(page.total).toBe(118);
  expect((await store.emails(ACCOUNT, ["e120"])).size).toBe(0);
});

test("in a list shorter than a page every row the server adds is new mail", async () => {
  const { server, client, store } = await serve(5);
  server.addEmail(email("e0", { receivedAt: at(0) }));
  await applyChanges(client, store, ["inbox"]);
  const page = await queryWindow(client, store, "inbox", 0);
  expect(page.pending).toBe(1);
  expect(page.ids).toHaveLength(5);
});

test("a state past the horizon drops the emails, threads and lists for a fresh fetch", async () => {
  const { server, client, store } = await serve(5);
  server.amend("e5", { keywords: { $seen: true } });
  server.forget();
  const applied = await applyChanges(client, store, ["inbox"]);
  expect(applied.windows).toEqual(["inbox"]);
  expect(await store.query(ACCOUNT, "inbox")).toBeNull();
  expect((await store.emails(ACCOUNT, ["e5"])).size).toBe(0);
  expect(await store.state(ACCOUNT, "Email")).toBeNull();
  expect(await store.state(ACCOUNT, "Thread")).toBeNull();
  expect(await store.state(ACCOUNT, "Mailbox")).toBe(String(server.sequence));
  const page = await queryWindow(client, store, "inbox", 0);
  expect(page.ids).toHaveLength(5);
  expect(page.emails["e5"]?.keywords).toEqual({ $seen: true });
  expect(await store.state(ACCOUNT, "Email")).toBe(String(server.sequence));
});

test("hasMoreChanges is followed within the rounds and the rest waits for the next poll", async () => {
  const { server, client, store } = await serve(12);
  server.changesCap = 1;
  for (let index = 1; index <= 12; index += 1) {
    server.amend(`e${String(index)}`, { keywords: { $seen: true } });
  }
  await applyChanges(client, store, ["inbox"]);
  expect(server.posted()).toHaveLength(CHANGES_ROUNDS_MAX);
  const seen = await store.emails(ACCOUNT, ["e8", "e9"]);
  expect(seen.get("e8")?.keywords).toEqual({ $seen: true });
  expect(seen.get("e9")?.keywords).toEqual({});
  await applyChanges(client, store, ["inbox"]);
  expect((await store.emails(ACCOUNT, ["e12"])).get("e12")?.keywords).toEqual({ $seen: true });
  expect(await store.state(ACCOUNT, "Email")).toBe(String(server.sequence));
});

test("a list nobody watches is dropped once it may have moved; a watched one is refreshed", async () => {
  const { server, client, store } = await serve(5);
  server.addEmail(email("a1", { mailboxIds: ["archive"], receivedAt: at(50) }));
  await queryWindow(client, store, "archive", 0);
  server.addEmail(email("e6", { receivedAt: at(60) }));
  const applied = await applyChanges(client, store, ["inbox"]);
  expect(applied.windows.toSorted()).toEqual(["archive", "inbox"]);
  expect(await store.query(ACCOUNT, "archive")).toBeNull();
  expect((await store.query(ACCOUNT, "inbox"))?.pending).toEqual(["e6"]);
});

test("a mailbox change lands without touching the lists", async () => {
  const { server, client, store } = await serve(5);
  server.putMailbox({ ...mailbox("inbox", "inbox"), unreadEmails: 3 });
  const applied = await applyChanges(client, store, ["inbox"]);
  expect(applied).toEqual({ mailboxes: true, windows: [], threads: [] });
  expect(server.posted()).toHaveLength(1);
  expect((await store.mailboxes(ACCOUNT)).find((row) => row.id === "inbox")?.unreadEmails).toBe(3);
});

test("an updated thread whose members answer is too large is rebuilt in chunks", async () => {
  const { server, client, store } = await serve(5);
  // A server that lowers a limit moves its session state with it; the
  // five exemplars still fit, the members of the thread do not.
  server.maxObjectsInGet = 6;
  server.sessionState = "s2";
  const replies = [6, 7, 8, 9, 10, 11, 12].map((second) => `e${String(second)}`);
  for (const [index, id] of replies.entries()) {
    server.addEmail(email(id, { threadId: "t-e5", receivedAt: at(6 + index) }));
  }
  await applyChanges(client, store, ["inbox"]);
  const thread = (await store.threads(ACCOUNT, ["t-e5"])).get("t-e5");
  expect(thread?.emailIds).toEqual(["e5", ...replies]);
  expect(Object.keys(thread?.members ?? {})).toEqual(["e5", ...replies]);
  const chunked = server.posted().filter((calls) => calls.every(([name]) => name === "Email/get"));
  expect(chunked.length).toBeGreaterThan(0);
});
