// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { JmapClient } from "../jmap/client";
import { z } from "../schema";
import { ACCOUNT, FakeJmap, UPSTREAM, at, email, json, mailbox } from "./fake-jmap";
import { CHANGES_ROUNDS_MAX } from "./limits";
import { syncMailboxes } from "./mailboxes";
import { MemoryMailStore } from "./memory";
import { applyPatch, flushPending } from "./pending";
import type { Mutation } from "./pending";
import { applyChanges } from "./poll";
import { listPage } from "./rows";
import { readThread } from "./thread";
import { queryWindow } from "./window";

interface Rig {
  server: FakeJmap;
  client: JmapClient;
  store: MemoryMailStore;
}

const SEEN = "keywords/$seen";
const NOTHING = { mailboxes: false, windows: [], threads: [] };
// A flush that settled its round without a refusal and left no row.
const SETTLED = { changes: NOTHING, failed: [], more: false };
const updateSchema = z.object({ update: z.record(z.string(), z.unknown()) });

function read(id: string): Mutation {
  return { type: "Email", id, patch: { [SEEN]: true } };
}

// A thread of two unread emails, an unread one and a read one, all in
// the inbox; the tree, the first page and the thread are held.
async function serve(knobs: (server: FakeJmap) => void = () => undefined): Promise<Rig> {
  const server = new FakeJmap();
  server.putMailbox(mailbox("inbox", "inbox"));
  server.addEmail(email("e1", { threadId: "t1", receivedAt: at(1) }));
  server.addEmail(email("e2", { threadId: "t1", receivedAt: at(2) }));
  server.addEmail(email("e3", { receivedAt: at(3) }));
  server.addEmail(email("e4", { receivedAt: at(4), keywords: ["$seen"] }));
  server.recount();
  knobs(server);
  vi.stubGlobal("fetch", server.fetch);
  const rig = { server, client: new JmapClient(ACCOUNT), store: new MemoryMailStore() };
  await syncMailboxes(rig.client, rig.store);
  await queryWindow(rig.client, rig.store, "inbox", 0);
  await readThread(rig.client, rig.store, "t1");
  server.requests.length = 0;
  return rig;
}

// The unread emails and threads the store counts in the inbox.
async function unread(store: MemoryMailStore): Promise<[number, number]> {
  const inbox = (await store.mailboxes(ACCOUNT)).find((row) => row.id === "inbox");
  return [inbox?.unreadEmails ?? -1, inbox?.unreadThreads ?? -1];
}

async function keywords(store: MemoryMailStore, id: string): Promise<unknown> {
  return (await store.emails(ACCOUNT, [id])).get(id)?.keywords;
}

// The email ids each Email/set sent so far named.
function updates(server: FakeJmap): string[][] {
  return server
    .posted()
    .flat()
    .filter(([name]) => name === "Email/set")
    .map(([, args]) => Object.keys(updateSchema.parse(args).update));
}

// An Email/set answered with a method error, for a canned response.
function failed(type: string): Response {
  return json(200, { methodResponses: [["error", { type }, "s"]], sessionState: "s1" });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("a mark as read moves the header, the thread member and the unread counts at once and sends nothing", async () => {
  const { server, client, store } = await serve();
  expect(await unread(store)).toEqual([3, 2]);
  const first = await applyPatch(store, ACCOUNT, read("e1"));
  expect(first).toEqual({ mailboxes: true, windows: ["inbox"], threads: ["t1"] });
  expect(await keywords(store, "e1")).toEqual({ $seen: true });
  const thread = (await store.threads(ACCOUNT, ["t1"])).get("t1");
  expect(thread?.members["e1"]?.keywords).toEqual({ $seen: true });
  expect(thread?.members["e2"]?.keywords).toEqual({});
  // The thread stays unread while its other member is.
  expect(await unread(store)).toEqual([2, 2]);
  await applyPatch(store, ACCOUNT, read("e2"));
  expect(await unread(store)).toEqual([1, 1]);
  const page = listPage(await queryWindow(client, store, "inbox", 0), "inbox");
  expect(page.rows.find((row) => row.threadId === "t1")?.unread).toBe(false);
  expect(await store.pending(ACCOUNT)).toEqual([
    {
      seq: 1,
      type: "Email",
      id: "e1",
      patch: { [SEEN]: true },
      inverse: { [SEEN]: null },
      sentAt: null,
    },
    {
      seq: 2,
      type: "Email",
      id: "e2",
      patch: { [SEEN]: true },
      inverse: { [SEEN]: null },
      sentAt: null,
    },
  ]);
  expect(server.requests).toHaveLength(0);
});

test("a change that moves nothing or names an email the store lacks is not logged", async () => {
  const { store } = await serve();
  expect(await applyPatch(store, ACCOUNT, read("e4"))).toEqual(NOTHING);
  expect(await applyPatch(store, ACCOUNT, read("e9"))).toEqual(NOTHING);
  expect(await store.pending(ACCOUNT)).toEqual([]);
  expect(await unread(store)).toEqual([3, 2]);
});

test("a patch that names anything but a keyword is refused before a row moves", async () => {
  const { store } = await serve();
  const moved: Mutation = { type: "Email", id: "e1", patch: { "mailboxIds/archive": true } };
  await expect(applyPatch(store, ACCOUNT, moved)).rejects.toThrow("keywords alone");
  const bare: Mutation = { type: "Email", id: "e1", patch: { "keywords/": true } };
  await expect(applyPatch(store, ACCOUNT, bare)).rejects.toThrow("keywords alone");
  expect(await store.pending(ACCOUNT)).toEqual([]);
  expect(await keywords(store, "e1")).toEqual({});
});

test("a flush sends the log as one Email/set that names no state and an acknowledged patch leaves it (RFC 8620 section 5.3)", async () => {
  const { server, client, store } = await serve();
  await applyPatch(store, ACCOUNT, read("e1"));
  await applyPatch(store, ACCOUNT, read("e2"));
  const flushed = await flushPending(client, store);
  expect(flushed).toEqual(SETTLED);
  expect(server.posted()).toEqual([
    [
      [
        "Email/set",
        {
          accountId: UPSTREAM,
          ifInState: null,
          update: { e1: { [SEEN]: true }, e2: { [SEEN]: true } },
        },
        "s",
      ],
    ],
  ]);
  expect(await store.pending(ACCOUNT)).toEqual([]);
  expect(server.emails.get("e1")?.keywords).toEqual({ $seen: true });
  // The server's own word for the write lands on rows that hold it already.
  await applyChanges(client, store, ["inbox"]);
  expect(await unread(store)).toEqual([1, 1]);
  expect(await keywords(store, "e2")).toEqual({ $seen: true });
  expect(await flushPending(client, store)).toEqual(SETTLED);
});

test("two patches of one email go out as one update, the later one winning a path", async () => {
  const { server, client, store } = await serve();
  await applyPatch(store, ACCOUNT, {
    type: "Email",
    id: "e3",
    patch: { [SEEN]: true, "keywords/$flagged": true },
  });
  await applyPatch(store, ACCOUNT, {
    type: "Email",
    id: "e3",
    patch: { "keywords/$flagged": null },
  });
  await flushPending(client, store);
  expect(server.posted()[0]?.[0]?.[1]).toMatchObject({
    update: { e3: { [SEEN]: true, "keywords/$flagged": null } },
  });
  expect(server.emails.get("e3")?.keywords).toEqual({ $seen: true });
  expect(await store.pending(ACCOUNT)).toEqual([]);
});

test("an update the server refuses is taken back from the rows and named; the acknowledged one stands", async () => {
  const { server, client, store } = await serve();
  server.refused.set("e1", "serverFail");
  await applyPatch(store, ACCOUNT, read("e1"));
  await applyPatch(store, ACCOUNT, read("e2"));
  const flushed = await flushPending(client, store);
  expect(flushed).toEqual({
    changes: { mailboxes: true, windows: ["inbox"], threads: ["t1"] },
    failed: ["e1"],
    more: false,
  });
  expect(await keywords(store, "e1")).toEqual({});
  expect(await keywords(store, "e2")).toEqual({ $seen: true });
  const thread = (await store.threads(ACCOUNT, ["t1"])).get("t1");
  expect(thread?.members["e1"]?.keywords).toEqual({});
  expect(await unread(store)).toEqual([2, 2]);
  expect(await store.pending(ACCOUNT)).toEqual([]);
});

test("a method error takes back every patch of the call", async () => {
  const { server, client, store } = await serve();
  await applyPatch(store, ACCOUNT, read("e1"));
  await applyPatch(store, ACCOUNT, read("e3"));
  server.readOnly = true;
  const flushed = await flushPending(client, store);
  expect(flushed.failed).toEqual(["e1", "e3"]);
  expect(flushed.changes.threads.toSorted()).toEqual(["t-e3", "t1"]);
  expect(await keywords(store, "e1")).toEqual({});
  expect(await keywords(store, "e3")).toEqual({});
  expect(await unread(store)).toEqual([3, 2]);
  expect(await store.pending(ACCOUNT)).toEqual([]);
});

test("the patch of an email that is gone leaves the log and nobody is told", async () => {
  const { server, client, store } = await serve();
  await applyPatch(store, ACCOUNT, read("e3"));
  server.destroyEmail("e3");
  expect((await flushPending(client, store)).failed).toEqual([]);
  expect(await store.pending(ACCOUNT)).toEqual([]);
});

test("a request that brings no answer leaves the log and the rows as they are and the next flush sends it again", async () => {
  const { server, client, store } = await serve();
  await applyPatch(store, ACCOUNT, read("e3"));
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("down")));
  await expect(flushPending(client, store)).rejects.toMatchObject({ code: "unavailable" });
  vi.stubGlobal("fetch", server.fetch);
  const problem = { type: "urn:ietf:params:jmap:error:limit", limit: "maxConcurrentRequests" };
  server.queue.push(json(400, problem, "application/problem+json"));
  await expect(flushPending(client, store)).rejects.toMatchObject({ code: "limit" });
  expect(await keywords(store, "e3")).toEqual({ $seen: true });
  expect(await unread(store)).toEqual([2, 1]);
  const waiting = await store.pending(ACCOUNT);
  expect(waiting).toHaveLength(1);
  expect(waiting[0]?.sentAt).toEqual(expect.any(Number));
  expect(await flushPending(client, store)).toEqual(SETTLED);
  expect(server.emails.get("e3")?.keywords).toEqual({ $seen: true });
  expect(await store.pending(ACCOUNT)).toEqual([]);
});

test("a call answered serverUnavailable leaves the log and the rows as they are and the next flush sends it again (RFC 8620 section 3.6.2)", async () => {
  const { server, client, store } = await serve();
  await applyPatch(store, ACCOUNT, read("e1"));
  server.queue.push(failed("serverUnavailable"));
  await expect(flushPending(client, store)).rejects.toMatchObject({
    name: "JmapError",
    code: "unavailable",
  });
  expect(await keywords(store, "e1")).toEqual({ $seen: true });
  const thread = (await store.threads(ACCOUNT, ["t1"])).get("t1");
  expect(thread?.members["e1"]?.keywords).toEqual({ $seen: true });
  expect(await unread(store)).toEqual([2, 2]);
  const waiting = await store.pending(ACCOUNT);
  expect(waiting).toHaveLength(1);
  expect(waiting[0]).toMatchObject({
    id: "e1",
    patch: { [SEEN]: true },
    inverse: { [SEEN]: null },
  });
  expect(waiting[0]?.sentAt).toEqual(expect.any(Number));
  expect(await flushPending(client, store)).toEqual(SETTLED);
  expect(server.emails.get("e1")?.keywords).toEqual({ $seen: true });
  expect(await store.pending(ACCOUNT)).toEqual([]);
});

test("more pending emails than one set takes go out in rounds", async () => {
  const { server, client, store } = await serve((knobs) => {
    knobs.maxObjectsInSet = 2;
  });
  await applyPatch(store, ACCOUNT, read("e1"));
  await applyPatch(store, ACCOUNT, read("e2"));
  await applyPatch(store, ACCOUNT, read("e3"));
  server.refused.set("e2", "forbidden");
  // A round answers for itself, so a later one that fails takes nothing from it.
  expect(await flushPending(client, store)).toMatchObject({ failed: ["e2"], more: true });
  expect((await store.pending(ACCOUNT)).map((row) => row.id)).toEqual(["e3"]);
  expect(await flushPending(client, store)).toEqual(SETTLED);
  expect(updates(server)).toEqual([["e1", "e2"], ["e3"]]);
  expect(await store.pending(ACCOUNT)).toEqual([]);
});

test("an update answered serverUnavailable keeps its rows at the head of the log and the round asks for no next one (RFC 8620 section 3.6.2)", async () => {
  const { server, client, store } = await serve((knobs) => {
    knobs.maxObjectsInSet = 2;
  });
  server.refused.set("e1", "serverUnavailable");
  await applyPatch(store, ACCOUNT, read("e1"));
  await applyPatch(store, ACCOUNT, read("e2"));
  await applyPatch(store, ACCOUNT, read("e3"));
  // The acknowledged one leaves; e3 waits past the round and is not asked for.
  expect(await flushPending(client, store)).toEqual(SETTLED);
  expect(await keywords(store, "e1")).toEqual({ $seen: true });
  expect(await unread(store)).toEqual([0, 0]);
  expect((await store.pending(ACCOUNT)).map((row) => row.id)).toEqual(["e1", "e3"]);
  expect(server.emails.get("e2")?.keywords).toEqual({ $seen: true });
  server.refused.delete("e1");
  expect(await flushPending(client, store)).toEqual(SETTLED);
  expect(updates(server)).toEqual([
    ["e1", "e2"],
    ["e1", "e3"],
  ]);
  expect(await store.pending(ACCOUNT)).toEqual([]);
});

test("a mailbox that lands a round ahead of the email whose lost write it counts is set right when the email lands", async () => {
  const { server, client, store } = await serve();
  await applyPatch(store, ACCOUNT, read("e3"));
  // The server took the write; an older change keeps the email out of the first round.
  server.amend("e4", { keywords: { $seen: true, $flagged: true } });
  server.amend("e3", { keywords: { $seen: true } });
  server.recount();
  server.changesCap = 1;
  await applyChanges(client, store, ["inbox"]);
  expect(server.posted()).toHaveLength(2);
  expect(await unread(store)).toEqual([2, 1]);
  expect((await store.pending(ACCOUNT)).map((row) => row.inverse)).toEqual([{ [SEEN]: true }]);
});

test("a mailbox set right by an email that lands a poll later is named as changed", async () => {
  const { server, client, store } = await serve();
  await applyPatch(store, ACCOUNT, read("e3"));
  // The server took the write; older changes keep the email out of the first poll.
  for (let round = 0; round < CHANGES_ROUNDS_MAX; round += 1) {
    server.amend("e4", { keywords: { $seen: true, $flagged: true } });
  }
  server.amend("e3", { keywords: { $seen: true } });
  server.recount();
  server.changesCap = 1;
  await applyChanges(client, store, ["inbox"]);
  // The mailbox landed ahead of the email, so it stands one low until then.
  expect(await unread(store)).toEqual([1, 0]);
  const applied = await applyChanges(client, store, ["inbox"]);
  expect(applied.mailboxes).toBe(true);
  expect(await unread(store)).toEqual([2, 1]);
});

test("a list fetched after a lost write keeps the count, which already says what the server holds", async () => {
  const { server, client, store } = await serve();
  await applyPatch(store, ACCOUNT, read("e3"));
  server.amend("e3", { keywords: { $seen: true } });
  server.recount();
  // The fetch lands the header as read; the mailbox row is still the old one.
  await store.commit(ACCOUNT, { queries: { remove: ["inbox"] } });
  const page = await queryWindow(client, store, "inbox", 0);
  expect(page.emails["e3"]?.keywords).toEqual({ $seen: true });
  expect((await store.pending(ACCOUNT)).map((row) => row.inverse)).toEqual([{ [SEEN]: true }]);
  expect(await unread(store)).toEqual([2, 1]);
  const applied = await applyChanges(client, store, ["inbox"]);
  expect(applied.mailboxes).toBe(true);
  expect(await unread(store)).toEqual([2, 1]);
});

test("an email that lands a round ahead of its mailbox leaves the count right once the mailbox lands", async () => {
  const { server, client, store } = await serve();
  await applyPatch(store, ACCOUNT, read("e3"));
  // An older mailbox change keeps the inbox out of the first round.
  server.putMailbox(mailbox("archive", "archive"));
  server.amend("e3", { keywords: { $seen: true } });
  server.recount();
  server.changesCap = 1;
  await applyChanges(client, store, ["inbox"]);
  expect(server.posted()).toHaveLength(2);
  expect(await unread(store)).toEqual([2, 1]);
});

test("a poll that lands an email as the server still holds it does not flip it back", async () => {
  const { server, client, store } = await serve();
  await applyPatch(store, ACCOUNT, read("e3"));
  // Another client flags the email; the server has not seen the patch.
  server.amend("e3", { keywords: { $flagged: true } });
  const applied = await applyChanges(client, store, ["inbox"]);
  expect(applied.threads).toEqual(["t-e3"]);
  expect(await keywords(store, "e3")).toEqual({ $flagged: true, $seen: true });
  const thread = (await store.threads(ACCOUNT, ["t-e3"])).get("t-e3");
  expect(thread?.members["e3"]?.keywords).toEqual({ $flagged: true, $seen: true });
  // No mailbox row landed, so the count holds the move it took.
  expect(await unread(store)).toEqual([2, 1]);
});

test("a mailbox row that lands counts the pending patches in again, a thread once", async () => {
  const { server, client, store } = await serve();
  await applyPatch(store, ACCOUNT, read("e1"));
  await applyPatch(store, ACCOUNT, read("e2"));
  const inbox = server.mailboxes.get("inbox");
  server.putMailbox({ ...mailbox("inbox", "inbox"), ...inbox, name: "Post" });
  await applyChanges(client, store, ["inbox"]);
  const held = (await store.mailboxes(ACCOUNT)).find((row) => row.id === "inbox");
  expect(held).toMatchObject({ name: "Post", unreadEmails: 1, unreadThreads: 1 });
  expect(server.mailboxes.get("inbox")).toMatchObject({ unreadEmails: 3, unreadThreads: 2 });
});

test("the tree fetched anew counts the pending patches in", async () => {
  const { client, store } = await serve();
  await applyPatch(store, ACCOUNT, read("e3"));
  await store.commit(ACCOUNT, { states: { Mailbox: null } });
  await syncMailboxes(client, store);
  expect(await unread(store)).toEqual([2, 1]);
});

test("a write the server took whose answer was lost is counted once", async () => {
  const { server, client, store } = await serve();
  await applyPatch(store, ACCOUNT, read("e3"));
  // The server holds the keyword and its counts say so; the log does not know.
  server.amend("e3", { keywords: { $seen: true } });
  server.recount();
  await applyChanges(client, store, ["inbox"]);
  expect(await unread(store)).toEqual([2, 1]);
  // Taking the patch back restores what the server said last.
  expect((await store.pending(ACCOUNT)).map((row) => row.inverse)).toEqual([{ [SEEN]: true }]);
  server.refused.set("e3", "forbidden");
  expect((await flushPending(client, store)).failed).toEqual(["e3"]);
  expect(await keywords(store, "e3")).toEqual({ $seen: true });
  expect(await unread(store)).toEqual([2, 1]);
});

test("a window and a thread fetched while a patch waits take the patch", async () => {
  const { client, store } = await serve();
  await applyPatch(store, ACCOUNT, read("e1"));
  await applyPatch(store, ACCOUNT, read("e3"));
  await store.commit(ACCOUNT, { reset: ["emails", "threads", "queries"] });
  const page = await queryWindow(client, store, "inbox", 0);
  expect(page.emails["e3"]?.keywords).toEqual({ $seen: true });
  expect(page.threads["t1"]?.members["e1"]?.keywords).toEqual({ $seen: true });
  expect(page.threads["t1"]?.members["e2"]?.keywords).toEqual({});
  const detail = await readThread(client, store, "t1");
  expect(detail?.emails["e1"]?.keywords).toEqual({ $seen: true });
  expect(await store.pending(ACCOUNT)).toHaveLength(2);
});

test("the first page refreshed by a poll while a patch waits keeps the patch", async () => {
  const { server, client, store } = await serve();
  await applyPatch(store, ACCOUNT, read("e3"));
  server.addEmail(email("e5", { receivedAt: at(5), keywords: ["$seen"] }));
  const applied = await applyChanges(client, store, ["inbox"]);
  expect(applied.windows).toEqual(["inbox"]);
  // The refresh put the page's headers as the server holds them.
  expect(server.emails.get("e3")?.keywords).toEqual({});
  expect(await keywords(store, "e3")).toEqual({ $seen: true });
  const thread = (await store.threads(ACCOUNT, ["t-e3"])).get("t-e3");
  expect(thread?.members["e3"]?.keywords).toEqual({ $seen: true });
});
