// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { JmapClient } from "../jmap/client";
import { ACCOUNT, FakeJmap, UPSTREAM, at, email, json, mailbox } from "./fake-jmap";
import { jobRound, jobStarted, runJob } from "./jobs";
import type { Job, JobProgress, JobRound } from "./jobs";
import { syncMailboxes } from "./mailboxes";
import { MemoryMailStore } from "./memory";
import { mutateMany } from "./pending";
import { readThread } from "./thread";
import { queryWindow } from "./window";

interface Rig {
  server: FakeJmap;
  client: JmapClient;
  store: MemoryMailStore;
}

const MARK_READ: Job = { kind: "markRead", mailboxId: "inbox" };
const EMPTY_TRASH: Job = { kind: "empty", mailboxId: "trash" };

// Five unread emails in the inbox, e1 the oldest and e2 read; three in
// Trash, t3 also filed in the archive. Each set takes two emails.
async function serve(): Promise<Rig> {
  const server = new FakeJmap();
  server.putMailbox(mailbox("inbox", "inbox"));
  server.putMailbox(mailbox("trash", "trash"));
  server.putMailbox(mailbox("archive", "archive"));
  for (let index = 1; index <= 5; index += 1) {
    const keywords = index === 2 ? ["$seen"] : [];
    server.addEmail(email(`e${String(index)}`, { receivedAt: at(index), keywords }));
  }
  server.addEmail(email("t1", { receivedAt: at(11), mailboxIds: ["trash"] }));
  server.addEmail(email("t2", { receivedAt: at(12), mailboxIds: ["trash"] }));
  server.addEmail(email("t3", { receivedAt: at(13), mailboxIds: ["trash", "archive"] }));
  server.recount();
  server.maxObjectsInSet = 2;
  vi.stubGlobal("fetch", server.fetch);
  const rig = { server, client: new JmapClient(ACCOUNT), store: new MemoryMailStore() };
  await syncMailboxes(rig.client, rig.store);
  await queryWindow(rig.client, rig.store, "inbox", 0);
  await queryWindow(rig.client, rig.store, "trash", 0);
  rig.server.requests.length = 0;
  return rig;
}

// One round on the rig, with no lock to hold.
function roundOn(rig: Rig): (progress: JobProgress) => Promise<JobRound> {
  return (progress) => jobRound(rig.client, rig.store, progress);
}

// The emails of a mailbox the server holds unread.
function unreadIn(server: FakeJmap, mailboxId: string): string[] {
  return [...server.emails.values()]
    .filter((row) => mailboxId in row.mailboxIds && !("$seen" in row.keywords))
    .map((row) => row.id);
}

async function counts(store: MemoryMailStore, id: string): Promise<number[]> {
  const row = (await store.mailboxes(ACCOUNT)).find((held) => held.id === id);
  return row === undefined
    ? []
    : [row.totalEmails, row.unreadEmails, row.totalThreads, row.unreadThreads];
}

// An answer that takes every update of a set while the server moves
// nothing: its state stands and no mailbox changed.
function takenUnapplied(server: FakeJmap, ids: readonly string[]): Response {
  const state = String(server.sequence);
  const states = { accountId: UPSTREAM, oldState: state, newState: state };
  const none = { accountId: UPSTREAM, state, list: [], notFound: [] };
  return json(200, {
    methodResponses: [
      ["Email/set", { ...states, updated: Object.fromEntries(ids.map((id) => [id, null])) }, "s"],
      [
        "Mailbox/changes",
        { ...states, hasMoreChanges: false, created: [], updated: [], destroyed: [] },
        "c:Mailbox",
      ],
      ["Mailbox/get", none, "n:Mailbox"],
      ["Mailbox/get", none, "u:Mailbox"],
    ],
    sessionState: server.sessionState,
  });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("mark all as read runs round after round of the set's size, the unread asked by notKeyword (RFC 8621 section 4.4.1)", async () => {
  const rig = await serve();
  const told: JobRound[] = [];
  const end = await runJob(jobStarted(MARK_READ), {
    round: roundOn(rig),
    canceled: () => false,
    told: (round) => told.push(round),
  });
  expect(end).toEqual({ job: MARK_READ, done: 4, total: 4, last: "e3", state: "done" });
  expect(told.map((round) => [round.progress.done, round.progress.state])).toEqual([
    [2, "running"],
    [4, "running"],
    [4, "done"],
  ]);
  const posted = rig.server.posted();
  const queries = posted
    .flat()
    .filter(([name]) => name === "Email/query")
    .map(([, args]) => args);
  expect(queries[0]).toMatchObject({
    filter: { inMailbox: "inbox", notKeyword: "$seen" },
    position: 0,
    limit: 2,
    calculateTotal: true,
    collapseThreads: false,
  });
  expect(queries[1]).toMatchObject({ calculateTotal: false });
  // The write and the mailbox changes since the held state ride one request.
  expect(posted[1]?.map(([name, , id]) => `${name}#${id}`)).toEqual([
    "Email/set#s",
    "Mailbox/changes#c:Mailbox",
    "Mailbox/get#n:Mailbox",
    "Mailbox/get#u:Mailbox",
  ]);
  const whole = posted
    .flat()
    .filter(([name, args]) => name === "Mailbox/get" && args["ids"] === null);
  expect(whole).toEqual([]);
  expect(unreadIn(rig.server, "inbox")).toEqual([]);
  expect(unreadIn(rig.server, "trash")).toHaveLength(3);
  // What the server took landed: the headers read, the counts as it holds them.
  expect((await rig.store.emails(ACCOUNT, ["e5"])).get("e5")?.keywords).toEqual({ $seen: true });
  expect(await counts(rig.store, "inbox")).toEqual([5, 0, 5, 0]);
  expect(await rig.store.state(ACCOUNT, "Mailbox")).toBe(String(rig.server.sequence));
  expect(await rig.store.pending(ACCOUNT)).toEqual([]);
});

test("a cancel ends the job after the round that runs, with what it did kept", async () => {
  const rig = await serve();
  let rounds = 0;
  const end = await runJob(jobStarted(MARK_READ), {
    round: async (progress) => {
      rounds += 1;
      return jobRound(rig.client, rig.store, progress);
    },
    canceled: () => rounds >= 1,
    told: () => undefined,
  });
  expect(end).toEqual({ job: MARK_READ, done: 2, total: 4, last: "e5", state: "canceled" });
  expect(unreadIn(rig.server, "inbox")).toEqual(["e1", "e3"]);
});

test("a failed round ends the job as failed with the rounds before it kept and its error reaches the caller", async () => {
  const rig = await serve();
  const told: JobRound[] = [];
  let rounds = 0;
  const run = runJob(jobStarted(MARK_READ), {
    round: async (progress) => {
      rounds += 1;
      if (rounds === 2) {
        rig.server.queue.push(json(502, { error: "upstream_failed" }));
      }
      return jobRound(rig.client, rig.store, progress);
    },
    canceled: () => false,
    told: (round) => told.push(round),
  });
  await expect(run).rejects.toMatchObject({ name: "JmapError", code: "upstream" });
  expect(told.at(-1)?.progress).toEqual({
    job: MARK_READ,
    done: 2,
    total: 4,
    last: "e5",
    state: "failed",
  });
});

test("a round the server takes nothing of fails the job", async () => {
  const rig = await serve();
  for (const id of ["e5", "e4", "e3", "e1"]) {
    rig.server.refused.set(id, "forbidden");
  }
  await expect(jobRound(rig.client, rig.store, jobStarted(MARK_READ))).rejects.toMatchObject({
    name: "MethodFailure",
  });
});

test("emptying Trash destroys what sits there alone and takes Trash from an email filed elsewhere too (RFC 8621 section 4.6)", async () => {
  const rig = await serve();
  await readThread(rig.client, rig.store, "t-t1");
  const end = await runJob(jobStarted(EMPTY_TRASH), {
    round: roundOn(rig),
    canceled: () => false,
    told: () => undefined,
  });
  expect(end).toEqual({ job: EMPTY_TRASH, done: 3, total: 3, last: "t1", state: "done" });
  const sets = rig.server
    .posted()
    .flat()
    .filter(([name]) => name === "Email/set")
    .map(([, args]) => [args["destroy"], args["update"]]);
  expect(sets).toEqual([
    [["t2"], { t3: { "mailboxIds/trash": null } }],
    [["t1"], {}],
  ]);
  expect(rig.server.emails.get("t3")?.mailboxIds).toEqual({ archive: true });
  expect(rig.server.emails.has("t1")).toBe(false);
  expect((await rig.store.emails(ACCOUNT, ["t1", "t2"])).size).toBe(0);
  expect((await rig.store.threads(ACCOUNT, ["t-t1"])).size).toBe(0);
  expect((await rig.store.emails(ACCOUNT, ["t3"])).get("t3")?.mailboxIds).toEqual({
    archive: true,
  });
  const listed = await rig.store.query(ACCOUNT, "trash");
  expect([listed?.pages[0]?.ids, listed?.total]).toEqual([[], 0]);
  expect(await counts(rig.store, "trash")).toEqual([0, 0, 0, 0]);
  expect(await counts(rig.store, "archive")).toEqual([1, 1, 1, 1]);
});

test("a change still logged for an email a round lands is laid over what the server took", async () => {
  const rig = await serve();
  await mutateMany(rig.store, ACCOUNT, [
    { type: "Email", id: "e5", patch: { "keywords/$flagged": true } },
    { type: "Email", id: "e4", patch: { "keywords/$seen": true } },
  ]);
  await jobRound(rig.client, rig.store, jobStarted(MARK_READ));
  expect((await rig.store.emails(ACCOUNT, ["e5"])).get("e5")?.keywords).toEqual({
    $seen: true,
    $flagged: true,
  });
  // The inverse of a logged change follows the header the server now holds.
  expect((await rig.store.pending(ACCOUNT)).map((row) => row.inverse)).toEqual([
    { "keywords/$flagged": null },
    { "keywords/$seen": true },
  ]);
  // The counts are the server's; the logged read of e4 is in them already.
  expect(await counts(rig.store, "inbox")).toEqual([5, 2, 5, 2]);
});

test("a server that takes a write without applying it fails the job at the next round", async () => {
  const rig = await serve();
  // Every write is answered as taken while the emails stay unread.
  vi.stubGlobal("fetch", (input: RequestInfo | URL, init?: RequestInit) => {
    if (typeof init?.body === "string" && init.body.includes("Email/set")) {
      rig.server.queue.push(takenUnapplied(rig.server, ["e5", "e4"]));
    }
    return rig.server.fetch(input, init);
  });
  const told: JobRound[] = [];
  const run = runJob(jobStarted(MARK_READ), {
    round: roundOn(rig),
    canceled: () => false,
    told: (round) => told.push(round),
  });
  await expect(run).rejects.toMatchObject({ name: "MethodFailure", type: "serverFail", call: "q" });
  expect(
    told.map((round) => [round.progress.done, round.progress.last, round.progress.state]),
  ).toEqual([
    [2, "e5", "running"],
    [2, "e5", "failed"],
  ]);
  expect(unreadIn(rig.server, "inbox")).toHaveLength(4);
});

test("an account that takes no write runs no round", async () => {
  const rig = await serve();
  rig.server.maxObjectsInSet = 0;
  const client = new JmapClient(ACCOUNT);
  await expect(jobRound(client, rig.store, jobStarted(MARK_READ))).rejects.toMatchObject({
    code: "limit",
  });
  expect(rig.server.posted()).toEqual([]);
});
