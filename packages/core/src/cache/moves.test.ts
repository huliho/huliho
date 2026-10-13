// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { JmapClient } from "../jmap/client";
import { z } from "../schema";
import { ACCOUNT, FakeJmap, at, email, mailbox } from "./fake-jmap";
import { flushPending } from "./flush";
import { syncMailboxes } from "./mailboxes";
import { MemoryMailStore } from "./memory";
import { mutateMany } from "./pending";
import type { PatchMutation } from "./pending";
import { applyChanges } from "./poll";
import { listPage } from "./rows";
import { readThread } from "./thread";
import { queryWindow } from "./window";

interface Rig {
  server: FakeJmap;
  client: JmapClient;
  store: MemoryMailStore;
}

const setSchema = z.object({ update: z.record(z.string(), z.unknown()).optional() });

function move(id: string, from: string, to: string): PatchMutation {
  return { type: "Email", id, patch: { [`mailboxIds/${from}`]: null, [`mailboxIds/${to}`]: true } };
}

// The thread t1 of two unread emails, an unread e3 and a read e4 in the
// inbox, an unread e5 in the archive; both lists, the tree and t1 held.
async function serve(): Promise<Rig> {
  const server = new FakeJmap();
  server.putMailbox(mailbox("inbox", "inbox"));
  server.putMailbox(mailbox("archive", "archive"));
  server.addEmail(email("e1", { threadId: "t1", receivedAt: at(1) }));
  server.addEmail(email("e2", { threadId: "t1", receivedAt: at(2) }));
  server.addEmail(email("e3", { receivedAt: at(3) }));
  server.addEmail(email("e4", { receivedAt: at(4), keywords: ["$seen"] }));
  server.addEmail(email("e5", { receivedAt: at(5), mailboxIds: ["archive"] }));
  server.recount();
  vi.stubGlobal("fetch", server.fetch);
  const rig = { server, client: new JmapClient(ACCOUNT), store: new MemoryMailStore() };
  await syncMailboxes(rig.client, rig.store);
  await queryWindow(rig.client, rig.store, "inbox", 0);
  await queryWindow(rig.client, rig.store, "archive", 0);
  await readThread(rig.client, rig.store, "t1");
  server.requests.length = 0;
  return rig;
}

// The four counts of a mailbox as the store holds them.
async function counts(store: MemoryMailStore, id: string): Promise<number[]> {
  const row = (await store.mailboxes(ACCOUNT)).find((held) => held.id === id);
  return row === undefined
    ? []
    : [row.totalEmails, row.unreadEmails, row.totalThreads, row.unreadThreads];
}

// The first page of a list and its total as the store holds them.
async function list(store: MemoryMailStore, id: string): Promise<[string[], number | null]> {
  const row = await store.query(ACCOUNT, id);
  return [row?.pages.find((page) => page.page === 0)?.ids ?? [], row?.total ?? null];
}

// The email ids each Email/set sent so far updated.
function updated(server: FakeJmap): string[][] {
  return server
    .posted()
    .flat()
    .filter(([name]) => name === "Email/set")
    .map(([, args]) => Object.keys(setSchema.parse(args).update ?? {}));
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("an archived thread leaves every page of its list at once with the total and the four counts, and the archive gains no row", async () => {
  const { store } = await serve();
  expect(await list(store, "inbox")).toEqual([["e4", "e3", "e2"], 3]);
  expect(await counts(store, "inbox")).toEqual([4, 3, 3, 2]);
  expect(await counts(store, "archive")).toEqual([1, 1, 1, 1]);
  const logged = await mutateMany(store, ACCOUNT, [
    move("e1", "inbox", "archive"),
    move("e2", "inbox", "archive"),
  ]);
  expect(logged.changes).toEqual({
    mailboxes: true,
    windows: ["inbox", "archive"],
    threads: ["t1"],
  });
  expect(await list(store, "inbox")).toEqual([["e4", "e3"], 2]);
  expect(await list(store, "archive")).toEqual([["e5"], 1]);
  expect(await counts(store, "inbox")).toEqual([2, 1, 2, 1]);
  expect(await counts(store, "archive")).toEqual([3, 3, 2, 2]);
  const thread = (await store.threads(ACCOUNT, ["t1"])).get("t1");
  expect(thread?.members["e1"]?.mailboxIds).toEqual({ archive: true });
  expect((await store.emails(ACCOUNT, ["e2"])).get("e2")?.mailboxIds).toEqual({ archive: true });
  const rows = listPage(await queryWindow(new JmapClient(ACCOUNT), store, "inbox", 0), "inbox");
  expect(rows.rows.map((row) => row.id)).toEqual(["e4", "e3"]);
  expect(rows.total).toBe(2);
});

test("the inverse of each email restores, and the thread returns to its place by receivedAt", async () => {
  const { store } = await serve();
  const logged = await mutateMany(store, ACCOUNT, [
    move("e1", "inbox", "archive"),
    move("e2", "inbox", "archive"),
  ]);
  expect(logged.inverses).toEqual([
    {
      type: "Email",
      id: "e1",
      patch: { "mailboxIds/inbox": true, "mailboxIds/archive": null },
      restores: true,
    },
    {
      type: "Email",
      id: "e2",
      patch: { "mailboxIds/inbox": true, "mailboxIds/archive": null },
      restores: true,
    },
  ]);
  expect(logged.held).toEqual([]);
  await mutateMany(store, ACCOUNT, logged.inverses);
  expect(await list(store, "inbox")).toEqual([["e4", "e3", "e2"], 3]);
  expect(await list(store, "archive")).toEqual([["e5"], 1]);
  expect(await counts(store, "inbox")).toEqual([4, 3, 3, 2]);
  expect(await counts(store, "archive")).toEqual([1, 1, 1, 1]);
});

test("several emails log under one call with one inverse each; an email named twice answers one", async () => {
  const { store } = await serve();
  const logged = await mutateMany(store, ACCOUNT, [
    { type: "Email", id: "e3", patch: { "keywords/$flagged": true } },
    { type: "Email", id: "e4", patch: { "keywords/$seen": null } },
    { type: "Email", id: "e3", patch: { "keywords/$seen": true, "keywords/$flagged": null } },
  ]);
  expect((await store.pending(ACCOUNT)).map((row) => [row.seq, row.id])).toEqual([
    [1, "e3"],
    [2, "e4"],
    [3, "e3"],
  ]);
  expect(logged.inverses).toEqual([
    {
      type: "Email",
      id: "e3",
      patch: { "keywords/$flagged": null, "keywords/$seen": null },
      restores: true,
    },
    { type: "Email", id: "e4", patch: { "keywords/$seen": true }, restores: true },
  ]);
  expect(await counts(store, "inbox")).toEqual([4, 3, 3, 2]);
});

test("a move and its undo logged before either went out send nothing (RFC 8620 section 5.3)", async () => {
  const { server, client, store } = await serve();
  const logged = await mutateMany(store, ACCOUNT, [move("e3", "inbox", "archive")]);
  await mutateMany(store, ACCOUNT, logged.inverses);
  expect(await flushPending(client, store)).toMatchObject({ failed: [], more: false });
  expect(server.posted()).toEqual([]);
  expect(await store.pending(ACCOUNT)).toEqual([]);
  expect(await list(store, "inbox")).toEqual([["e4", "e3", "e2"], 3]);
});

test("a change the server holds already before it went out leaves the log without a request", async () => {
  const { server, client, store } = await serve();
  await mutateMany(store, ACCOUNT, [
    { type: "Email", id: "e3", patch: { "keywords/$seen": true } },
  ]);
  // Another client read it first.
  server.amend("e3", { keywords: { $seen: true } });
  server.recount();
  await applyChanges(client, store, ["inbox"]);
  server.requests.length = 0;
  expect(await flushPending(client, store)).toMatchObject({ failed: [], more: false });
  expect(server.requests).toHaveLength(0);
  expect(await store.pending(ACCOUNT)).toEqual([]);
  expect((await store.emails(ACCOUNT, ["e3"])).get("e3")?.keywords).toEqual({ $seen: true });
  expect(await counts(store, "inbox")).toEqual([4, 2, 3, 1]);
});

test("a move goes out as a pair of paths, and an undo after it went out is sent in turn", async () => {
  const { server, client, store } = await serve();
  const logged = await mutateMany(store, ACCOUNT, [move("e3", "inbox", "archive")]);
  await flushPending(client, store);
  expect(server.posted()[0]?.[0]?.[1]).toMatchObject({
    update: { e3: { "mailboxIds/inbox": null, "mailboxIds/archive": true } },
  });
  expect(server.emails.get("e3")?.mailboxIds).toEqual({ archive: true });
  await mutateMany(store, ACCOUNT, logged.inverses);
  await flushPending(client, store);
  expect(updated(server)).toEqual([["e3"], ["e3"]]);
  expect(server.emails.get("e3")?.mailboxIds).toEqual({ inbox: true });
  expect(await list(store, "inbox")).toEqual([["e4", "e3", "e2"], 3]);
});

test("a poll before the server answers never puts an archived thread back", async () => {
  const { server, client, store } = await serve();
  await mutateMany(store, ACCOUNT, [move("e3", "inbox", "archive")]);
  server.addEmail(email("e6", { receivedAt: at(0), keywords: ["$seen"] }));
  const applied = await applyChanges(client, store, ["inbox"]);
  expect(applied.windows).toContain("inbox");
  // The fresh first page from the server still names e3.
  expect(await list(store, "inbox")).toEqual([["e4", "e2", "e6"], 3]);
  expect((await store.emails(ACCOUNT, ["e3"])).get("e3")?.mailboxIds).toEqual({ archive: true });
  await store.commit(ACCOUNT, { queries: { remove: ["inbox"] } });
  const page = await queryWindow(client, store, "inbox", 0);
  expect(page.ids).toEqual(["e4", "e2", "e6"]);
  expect(page.total).toBe(3);
});

test("a refused move restores the memberships and the thread returns to its place", async () => {
  const { server, client, store } = await serve();
  server.refused.set("e1", "forbidden");
  server.refused.set("e2", "forbidden");
  await mutateMany(store, ACCOUNT, [
    move("e1", "inbox", "archive"),
    move("e2", "inbox", "archive"),
  ]);
  const flushed = await flushPending(client, store);
  expect(flushed.failed).toEqual(["e1", "e2"]);
  expect(flushed.changes.windows.toSorted()).toEqual(["archive", "inbox"]);
  expect(await list(store, "inbox")).toEqual([["e4", "e3", "e2"], 3]);
  expect(await counts(store, "inbox")).toEqual([4, 3, 3, 2]);
  expect(await counts(store, "archive")).toEqual([1, 1, 1, 1]);
  expect(await store.pending(ACCOUNT)).toEqual([]);
});

test("a patch that would leave an email in no mailbox is refused by the server and taken back (RFC 8621 section 4.1.1)", async () => {
  const { client, store } = await serve();
  await mutateMany(store, ACCOUNT, [
    { type: "Email", id: "e3", patch: { "mailboxIds/inbox": null } },
  ]);
  expect(await list(store, "inbox")).toEqual([["e4", "e2"], 2]);
  expect((await flushPending(client, store)).failed).toEqual(["e3"]);
  expect(await list(store, "inbox")).toEqual([["e4", "e3", "e2"], 3]);
  expect(await counts(store, "inbox")).toEqual([4, 3, 3, 2]);
});

test("a move into a list never inserts a row under the pointer; the next poll brings it behind the marker", async () => {
  const { client, store } = await serve();
  await mutateMany(store, ACCOUNT, [move("e5", "archive", "inbox")]);
  expect(await list(store, "inbox")).toEqual([["e4", "e3", "e2"], 3]);
  expect(await list(store, "archive")).toEqual([[], 0]);
  expect(await counts(store, "inbox")).toEqual([5, 4, 4, 3]);
  await flushPending(client, store);
  await applyChanges(client, store, ["inbox"]);
  expect(await list(store, "inbox")).toEqual([["e4", "e3", "e2"], 3]);
  expect((await store.query(ACCOUNT, "inbox"))?.pending).toEqual(["e5"]);
});

test("a thread with a member left in the list keeps its row", async () => {
  const { store } = await serve();
  await mutateMany(store, ACCOUNT, [move("e1", "inbox", "archive")]);
  expect(await list(store, "inbox")).toEqual([["e4", "e3", "e2"], 3]);
  expect(await counts(store, "inbox")).toEqual([3, 2, 3, 2]);
  expect(await counts(store, "archive")).toEqual([2, 2, 2, 2]);
});
