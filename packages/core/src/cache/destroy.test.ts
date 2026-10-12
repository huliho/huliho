// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { JmapClient } from "../jmap/client";
import { readBody } from "./bodies";
import { ACCOUNT, FakeJmap, UPSTREAM, at, email, mailbox } from "./fake-jmap";
import { flushPending } from "./flush";
import { syncMailboxes } from "./mailboxes";
import { MemoryMailStore } from "./memory";
import { dropHeld, mutateMany, releaseHeld, releaseLapsed } from "./pending";
import type { DestroyMutation } from "./pending";
import { applyChanges } from "./poll";
import { readThread } from "./thread";
import { queryWindow } from "./window";

interface Rig {
  server: FakeJmap;
  client: JmapClient;
  store: MemoryMailStore;
}

// A round settled without a refusal and with no next one.
const SETTLED = { failed: [], more: false };

function destroy(id: string): DestroyMutation {
  return { type: "Email", id, destroy: true };
}

// In Trash the thread t1 of two unread emails and a read e3; e4 in the
// inbox. The tree, the Trash list and both threads are held.
async function serve(): Promise<Rig> {
  const server = new FakeJmap();
  server.putMailbox(mailbox("inbox", "inbox"));
  server.putMailbox(mailbox("trash", "trash"));
  server.addEmail(email("e1", { threadId: "t1", receivedAt: at(1), mailboxIds: ["trash"] }));
  server.addEmail(email("e2", { threadId: "t1", receivedAt: at(2), mailboxIds: ["trash"] }));
  server.addEmail(email("e3", { receivedAt: at(3), mailboxIds: ["trash"], keywords: ["$seen"] }));
  server.addEmail(email("e4", { receivedAt: at(4) }));
  server.recount();
  vi.stubGlobal("fetch", server.fetch);
  const rig = { server, client: new JmapClient(ACCOUNT), store: new MemoryMailStore() };
  await syncMailboxes(rig.client, rig.store);
  await queryWindow(rig.client, rig.store, "trash", 0);
  await readThread(rig.client, rig.store, "t1");
  await readThread(rig.client, rig.store, "t-e3");
  server.requests.length = 0;
  return rig;
}

// The first page of the Trash list, its total and the four counts.
async function trash(store: MemoryMailStore): Promise<[string[], number | null, number[]]> {
  const row = await store.query(ACCOUNT, "trash");
  const box = (await store.mailboxes(ACCOUNT)).find((held) => held.id === "trash");
  return [
    row?.pages[0]?.ids ?? [],
    row?.total ?? null,
    box === undefined
      ? []
      : [box.totalEmails, box.unreadEmails, box.totalThreads, box.unreadThreads],
  ];
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("a delete forever hides the email from the pages, the counts and its thread at once; the flush passes it over", async () => {
  const { server, client, store } = await serve();
  expect(await trash(store)).toEqual([["e3", "e2"], 2, [3, 2, 2, 1]]);
  const logged = await mutateMany(store, ACCOUNT, [destroy("e3")], "lease-1");
  expect(logged.inverses).toEqual([]);
  expect(logged.held).toEqual([1]);
  expect(await trash(store)).toEqual([["e2"], 1, [2, 2, 1, 1]]);
  expect(await readThread(client, store, "t-e3")).toBeNull();
  expect(await store.pending(ACCOUNT)).toEqual([
    {
      seq: 1,
      type: "Email",
      id: "e3",
      patch: { "mailboxIds/trash": null },
      inverse: { "mailboxIds/trash": true },
      sentAt: null,
      destroy: true,
      held: true,
      lease: "lease-1",
    },
  ]);
  expect(await flushPending(client, store)).toMatchObject(SETTLED);
  expect(server.requests).toHaveLength(0);
  expect(server.emails.has("e3")).toBe(true);
});

test("a thread answers without the member a delete forever hides", async () => {
  const { client, store } = await serve();
  await mutateMany(store, ACCOUNT, [destroy("e1")], "lease-1");
  const detail = await readThread(client, store, "t1");
  expect(detail?.thread.emailIds).toEqual(["e2"]);
  expect(Object.keys(detail?.thread.members ?? {})).toEqual(["e2"]);
  expect(Object.keys(detail?.emails ?? {})).toEqual(["e2"]);
  expect(await trash(store)).toEqual([["e3", "e2"], 2, [2, 1, 2, 1]]);
});

test("a released destroy goes out alone as one destroy and the email leaves the store with its body (RFC 8620 section 5.3)", async () => {
  const { server, client, store } = await serve();
  await readBody(client, store, "e3", { large: false });
  expect(await store.body(ACCOUNT, "e3")).not.toBeNull();
  const { held } = await mutateMany(store, ACCOUNT, [destroy("e3")], "lease-1");
  expect(await releaseHeld(store, ACCOUNT, held)).toBe(true);
  server.requests.length = 0;
  expect(await flushPending(client, store)).toMatchObject(SETTLED);
  expect(server.posted()).toEqual([
    [["Email/set", { accountId: UPSTREAM, ifInState: null, destroy: ["e3"] }, "s"]],
  ]);
  expect(server.emails.has("e3")).toBe(false);
  expect((await store.emails(ACCOUNT, ["e3"])).size).toBe(0);
  expect(await store.body(ACCOUNT, "e3")).toBeNull();
  expect((await store.threads(ACCOUNT, ["t-e3"])).size).toBe(0);
  expect(await store.pending(ACCOUNT)).toEqual([]);
  expect(await trash(store)).toEqual([["e2"], 1, [2, 2, 1, 1]]);
});

test("a change logged before the destroy leaves with it and is never sent beside it", async () => {
  const { server, client, store } = await serve();
  await mutateMany(store, ACCOUNT, [
    { type: "Email", id: "e1", patch: { "keywords/$seen": true } },
  ]);
  const { held } = await mutateMany(store, ACCOUNT, [destroy("e1")], "lease-1");
  await releaseHeld(store, ACCOUNT, held);
  await flushPending(client, store);
  expect(server.posted()[0]?.[0]?.[1]).toEqual({
    accountId: UPSTREAM,
    ifInState: null,
    destroy: ["e1"],
  });
  expect(await store.pending(ACCOUNT)).toEqual([]);
  expect((await readThread(client, store, "t1"))?.thread.emailIds).toEqual(["e2"]);
});

test("a destroy the server refuses brings the email back where it stood and is named", async () => {
  const { server, client, store } = await serve();
  server.refused.set("e3", "forbidden");
  const { held } = await mutateMany(store, ACCOUNT, [destroy("e3")], "lease-1");
  await releaseHeld(store, ACCOUNT, held);
  const flushed = await flushPending(client, store);
  expect(flushed.failed).toEqual(["e3"]);
  expect(flushed.changes.windows).toEqual(["trash"]);
  expect(await trash(store)).toEqual([["e3", "e2"], 2, [3, 2, 2, 1]]);
  expect((await store.emails(ACCOUNT, ["e3"])).get("e3")?.mailboxIds).toEqual({ trash: true });
  expect(await store.pending(ACCOUNT)).toEqual([]);
});

test("a refused move under a held destroy keeps the email hidden and the destroy refitted", async () => {
  const { server, client, store } = await serve();
  await readThread(client, store, "t-e4");
  await mutateMany(store, ACCOUNT, [
    { type: "Email", id: "e4", patch: { "mailboxIds/inbox": null, "mailboxIds/trash": true } },
  ]);
  await mutateMany(store, ACCOUNT, [destroy("e4")], "lease-1");
  server.refused.set("e4", "forbidden");
  expect((await flushPending(client, store)).failed).toEqual(["e4"]);
  expect((await store.emails(ACCOUNT, ["e4"])).get("e4")?.mailboxIds).toEqual({});
  expect(await store.pending(ACCOUNT)).toMatchObject([
    { id: "e4", destroy: true, held: true, inverse: { "mailboxIds/inbox": true } },
  ]);
});

test("a destroy of an email another client destroyed first leaves the store and nobody is told", async () => {
  const { server, client, store } = await serve();
  const { held } = await mutateMany(store, ACCOUNT, [destroy("e3")], "lease-1");
  server.destroyEmail("e3");
  await releaseHeld(store, ACCOUNT, held);
  expect(await flushPending(client, store)).toMatchObject(SETTLED);
  expect((await store.emails(ACCOUNT, ["e3"])).size).toBe(0);
  expect(await store.pending(ACCOUNT)).toEqual([]);
});

test("undo drops the held destroy: the email shows again at its place and nothing is sent", async () => {
  const { server, client, store } = await serve();
  const { held } = await mutateMany(store, ACCOUNT, [destroy("e3")], "lease-1");
  const changes = await dropHeld(store, ACCOUNT, held);
  expect(changes).toEqual({ mailboxes: true, windows: ["trash"], threads: ["t-e3"] });
  expect(await trash(store)).toEqual([["e3", "e2"], 2, [3, 2, 2, 1]]);
  expect((await readThread(client, store, "t-e3"))?.thread.emailIds).toEqual(["e3"]);
  expect(await store.pending(ACCOUNT)).toEqual([]);
  expect(await flushPending(client, store)).toMatchObject(SETTLED);
  expect(server.requests).toHaveLength(0);
});

test("a released destroy is past taking back", async () => {
  const { store } = await serve();
  const { held } = await mutateMany(store, ACCOUNT, [destroy("e3")], "lease-1");
  await releaseHeld(store, ACCOUNT, held);
  expect(await dropHeld(store, ACCOUNT, held)).toEqual({
    mailboxes: false,
    windows: [],
    threads: [],
  });
  expect((await store.pending(ACCOUNT)).map((row) => row.held)).toEqual([false]);
  expect(await releaseHeld(store, ACCOUNT, held)).toBe(false);
});

test("the held destroys of a lease that lapsed go out, the ones without a lease too; a live lease keeps its own", async () => {
  const { store } = await serve();
  await mutateMany(store, ACCOUNT, [destroy("e1")], "a");
  await mutateMany(store, ACCOUNT, [destroy("e2")], "b");
  await mutateMany(store, ACCOUNT, [destroy("e3")]);
  expect(await releaseLapsed(store, ACCOUNT, new Set(["a"]))).toBe(true);
  expect((await store.pending(ACCOUNT)).map((row) => [row.id, row.held])).toEqual([
    ["e1", true],
    ["e2", false],
    ["e3", false],
  ]);
  // A start with no tab attached yet releases every held one.
  expect(await releaseLapsed(store, ACCOUNT, new Set())).toBe(true);
  expect((await store.pending(ACCOUNT)).every((row) => row.held === false)).toBe(true);
});

test("an email that waits to be destroyed takes no other change", async () => {
  const { store } = await serve();
  await mutateMany(store, ACCOUNT, [destroy("e3")], "a");
  const logged = await mutateMany(store, ACCOUNT, [
    { type: "Email", id: "e3", patch: { "mailboxIds/trash": null, "mailboxIds/inbox": true } },
    destroy("e3"),
  ]);
  expect(logged.held).toEqual([]);
  expect(await store.pending(ACCOUNT)).toHaveLength(1);
});

test("a patch after a destroy of the same email in one call is not logged", async () => {
  const { store } = await serve();
  const logged = await mutateMany(
    store,
    ACCOUNT,
    [
      destroy("e3"),
      { type: "Email", id: "e3", patch: { "mailboxIds/trash": null, "mailboxIds/inbox": true } },
    ],
    "a",
  );
  expect(logged.held).toEqual([1]);
  expect(logged.inverses).toEqual([]);
  const pending = await store.pending(ACCOUNT);
  expect(pending.map((row) => [row.id, row.destroy])).toEqual([["e3", true]]);
  expect(await trash(store)).toEqual([["e2"], 1, [2, 2, 1, 1]]);
  const inbox = (await store.mailboxes(ACCOUNT)).find((box) => box.id === "inbox");
  expect(inbox).toMatchObject({ totalEmails: 1, totalThreads: 1 });
});

test("a poll while the destroy is held keeps the email out of the list and its thread", async () => {
  const { server, client, store } = await serve();
  await mutateMany(store, ACCOUNT, [destroy("e3")], "lease-1");
  // Another client flags the email; the server still holds it in Trash.
  server.amend("e3", { keywords: { $seen: true, $flagged: true } });
  await applyChanges(client, store, ["trash"]);
  expect(await trash(store)).toEqual([["e2"], 1, [2, 2, 1, 1]]);
  expect(await readThread(client, store, "t-e3")).toBeNull();
  await store.commit(ACCOUNT, { queries: { remove: ["trash"] } });
  expect((await queryWindow(client, store, "trash", 0)).ids).toEqual(["e2"]);
});
