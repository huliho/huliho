// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { JmapClient } from "../jmap/client";
import type { Mailbox } from "../jmap/schemas";
import { ACCOUNT, FakeJmap, at, email, mailbox } from "./fake-jmap";
import { flushPending } from "./flush";
import { syncMailboxes } from "./mailboxes";
import { MemoryMailStore } from "./memory";
import { mutateMany, releaseHeld } from "./pending";
import type { Mutation, PatchMutation } from "./pending";
import { applyChanges } from "./poll";
import { mayPatch } from "./rights";
import { queryWindow } from "./window";

interface Rig {
  server: FakeJmap;
  client: JmapClient;
  store: MemoryMailStore;
}

function move(id: string, from: string, to: string): PatchMutation {
  return {
    type: "Email",
    id,
    threadId: "t1",
    patch: { [`mailboxIds/${from}`]: null, [`mailboxIds/${to}`]: true },
  };
}

// A mailbox that takes and gives up emails, unless `adds` says otherwise.
function writable(id: string, role: string | null, adds = true): Mailbox {
  const row = mailbox(id, role);
  return { ...row, myRights: { ...row.myRights, mayAddItems: adds, mayRemoveItems: true } };
}

// The thread t1 of an unread e1 and an unread e2 in the inbox beside a
// read e3; t2 of e4 and e5 in Trash. Only the lists are held, so the
// store holds the header of each thread's newest email and the others
// stand in their thread rows alone.
async function serve(): Promise<Rig> {
  const server = new FakeJmap();
  server.putMailbox(writable("inbox", "inbox"));
  server.putMailbox(writable("archive", "archive"));
  server.putMailbox(writable("trash", "trash"));
  server.putMailbox(writable("folder", null));
  server.addEmail(email("e1", { threadId: "t1", receivedAt: at(1) }));
  server.addEmail(email("e2", { threadId: "t1", receivedAt: at(2) }));
  server.addEmail(email("e3", { receivedAt: at(3), keywords: ["$seen"] }));
  server.addEmail(email("e4", { threadId: "t2", receivedAt: at(4), mailboxIds: ["trash"] }));
  server.addEmail(email("e5", { threadId: "t2", receivedAt: at(5), mailboxIds: ["trash"] }));
  server.recount();
  vi.stubGlobal("fetch", server.fetch);
  const rig = { server, client: new JmapClient(ACCOUNT), store: new MemoryMailStore() };
  await syncMailboxes(rig.client, rig.store);
  await queryWindow(rig.client, rig.store, "inbox", 0);
  await queryWindow(rig.client, rig.store, "trash", 0);
  server.requests.length = 0;
  return rig;
}

// The first page of a list and its total.
async function list(store: MemoryMailStore, id: string): Promise<[string[], number | null]> {
  const row = await store.query(ACCOUNT, id);
  return [row?.pages[0]?.ids ?? [], row?.total ?? null];
}

// The four counts of a mailbox as the store holds them.
async function counts(store: MemoryMailStore, id: string): Promise<number[]> {
  const row = (await store.mailboxes(ACCOUNT)).find((held) => held.id === id);
  return row === undefined
    ? []
    : [row.totalEmails, row.unreadEmails, row.totalThreads, row.unreadThreads];
}

async function member(store: MemoryMailStore, threadId: string, id: string): Promise<unknown> {
  const thread = (await store.threads(ACCOUNT, [threadId])).get(threadId);
  return new Map(Object.entries(thread?.members ?? {})).get(id)?.mailboxIds;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("an email the store knows neither by its header nor in the thread named is never logged", async () => {
  const { store } = await serve();
  const unknown: Mutation[] = [
    { ...move("e1", "inbox", "archive"), threadId: "t2" },
    { type: "Email", id: "e1", patch: { "mailboxIds/inbox": null, "mailboxIds/archive": true } },
    { ...move("e9", "inbox", "archive") },
  ];
  expect((await mutateMany(store, ACCOUNT, unknown)).inverses).toEqual([]);
  expect(await store.pending(ACCOUNT)).toEqual([]);
  expect(await counts(store, "inbox")).toEqual([3, 2, 2, 1]);
});

test("a move into a mailbox without mayAddItems is refused for an email known by its thread alone", async () => {
  const { server, client, store } = await serve();
  server.putMailbox(writable("folder", null, false));
  await syncMailboxes(client, store);
  expect(await mayPatch(client, store, move("e1", "inbox", "folder"))).toBe(false);
  expect(await mayPatch(client, store, move("e1", "inbox", "archive"))).toBe(true);
  expect(await mayPatch(client, store, { ...move("e1", "inbox", "archive"), threadId: "t9" })).toBe(
    false,
  );
});

test("an archive of a thread takes the email the store knows by its thread alone, and the thread leaves", async () => {
  const { store } = await serve();
  expect((await store.emails(ACCOUNT, ["e1"])).has("e1")).toBe(false);
  const logged = await mutateMany(store, ACCOUNT, [
    move("e1", "inbox", "archive"),
    move("e2", "inbox", "archive"),
  ]);
  expect((await store.pending(ACCOUNT)).map((row) => [row.id, row.threadId])).toEqual([
    ["e1", "t1"],
    ["e2", "t1"],
  ]);
  expect(logged.inverses.map((inverse) => [inverse.id, inverse.threadId])).toEqual([
    ["e1", "t1"],
    ["e2", "t1"],
  ]);
  expect(await member(store, "t1", "e1")).toEqual({ archive: true });
  expect((await store.emails(ACCOUNT, ["e1"])).has("e1")).toBe(false);
  expect(await list(store, "inbox")).toEqual([["e3"], 1]);
  expect(await counts(store, "inbox")).toEqual([1, 0, 1, 0]);
  expect(await counts(store, "archive")).toEqual([2, 2, 1, 1]);
  await mutateMany(store, ACCOUNT, logged.inverses);
  expect(await list(store, "inbox")).toEqual([["e3", "e2"], 2]);
  expect(await counts(store, "inbox")).toEqual([3, 2, 2, 1]);
  expect(await member(store, "t1", "e1")).toEqual({ inbox: true });
});

test("the flush sends every email of the thread and leaves the log empty once acknowledged", async () => {
  const { server, client, store } = await serve();
  await mutateMany(store, ACCOUNT, [
    move("e1", "inbox", "archive"),
    move("e2", "inbox", "archive"),
  ]);
  expect(await flushPending(client, store)).toMatchObject({ failed: [], more: false });
  expect(server.emails.get("e1")?.mailboxIds).toEqual({ archive: true });
  expect(server.emails.get("e2")?.mailboxIds).toEqual({ archive: true });
  expect(await store.pending(ACCOUNT)).toEqual([]);
});

test("a refused move puts the member the store holds no header for back where the server holds it", async () => {
  const { server, client, store } = await serve();
  await mutateMany(store, ACCOUNT, [
    move("e1", "inbox", "archive"),
    move("e2", "inbox", "archive"),
  ]);
  server.refused.set("e1", "forbidden");
  expect(await flushPending(client, store)).toMatchObject({ failed: ["e1"] });
  expect(await member(store, "t1", "e1")).toEqual({ inbox: true });
  expect(await counts(store, "inbox")).toEqual([2, 1, 2, 1]);
  expect(await counts(store, "archive")).toEqual([1, 1, 1, 1]);
  expect(await store.pending(ACCOUNT)).toEqual([]);
});

test("a poll keeps the change over the member and refits its inverse to where the server moved it", async () => {
  const { server, client, store } = await serve();
  await mutateMany(store, ACCOUNT, [
    move("e1", "inbox", "archive"),
    move("e2", "inbox", "archive"),
  ]);
  server.amend("e1", { mailboxIds: { folder: true } });
  server.recount();
  await applyChanges(client, store, ["inbox"]);
  expect(await member(store, "t1", "e1")).toEqual({ folder: true, archive: true });
  expect(await list(store, "inbox")).toEqual([["e3"], 1]);
  const row = (await store.pending(ACCOUNT)).find((held) => held.id === "e1");
  expect(row?.inverse).toEqual({ "mailboxIds/inbox": null, "mailboxIds/archive": null });
  server.refused.set("e1", "forbidden");
  await flushPending(client, store);
  expect(await member(store, "t1", "e1")).toEqual({ folder: true });
});

test("a delete forever of a thread hides the email known by its thread alone and the destroy drops it from the thread", async () => {
  const { server, client, store } = await serve();
  const logged = await mutateMany(
    store,
    ACCOUNT,
    ["e4", "e5"].map((id): Mutation => ({ type: "Email", id, threadId: "t2", destroy: true })),
    "lease-1",
  );
  expect(logged.held).toEqual([1, 2]);
  expect(await member(store, "t2", "e4")).toEqual({});
  expect(await list(store, "trash")).toEqual([[], 0]);
  expect(await counts(store, "trash")).toEqual([0, 0, 0, 0]);
  await releaseHeld(store, ACCOUNT, logged.held);
  expect(await flushPending(client, store)).toMatchObject({ failed: [], more: false });
  expect(server.emails.has("e4")).toBe(false);
  expect((await store.threads(ACCOUNT, ["t2"])).has("t2")).toBe(false);
  expect(await store.pending(ACCOUNT)).toEqual([]);
});
