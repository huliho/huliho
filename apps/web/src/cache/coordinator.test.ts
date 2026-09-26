// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// @vitest-environment node

import { WINDOW_SIZE } from "@huliho/core";
import { ACCOUNT, FakeJmap, at, email, json, mailbox } from "@huliho/core/testing";
import { IDBFactory, IDBKeyRange } from "fake-indexeddb";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { CHANGES_POLL_MS, Coordinator, FIRST_SYNC_POLL_MS, LEASE_MS } from "./coordinator";
import type { CacheApi } from "./coordinator";
import { DexieMailStore, MailDatabase } from "./db";
import { webLocks } from "./locks";
import type { Locks } from "./locks";
import type { CacheMessage } from "./messages";

const LIMIT_PROBLEM = { type: "urn:ietf:params:jmap:error:limit", limit: "maxSizeRequest" };
const PROBLEM_TYPE = "application/problem+json";
const INBOX = { accountId: ACCOUNT, mailboxId: "inbox" };

// Only the clock is faked: IndexedDB runs on immediates and the fake
// server answers on real promises.
const FAKED = ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"] as const;

// How many turns of the event loop let the work of a fired timer finish.
const DRAIN_TURNS = 50;

// A step of the clock short of the next poll.
const BEFORE_POLL_MS = 1000;

// Longer than the focus gap and shorter than the poll interval.
const PAST_FOCUS_GAP_MS = 20_000;

const TREE_DROPPED = {
  kind: "changed",
  accountId: ACCOUNT,
  mailboxes: true,
  windows: [],
  threads: [],
};

interface Rig {
  server: FakeJmap;
  posted: CacheMessage[];
  coordinator: Coordinator;
  tab: CacheApi;
}

// One IndexedDB per test, shared by every coordinator of the test as the
// tabs of one browser share it; the locks are the platform's, under a
// name per test so a lock a test leaves held never reaches the next one.
let indexedDB = new IDBFactory();
let scope = "";

function store(): DexieMailStore {
  return new DexieMailStore(new MailDatabase("huliho-test", { indexedDB, IDBKeyRange }));
}

function locks(): Locks {
  const platform = webLocks(navigator.locks);
  return { request: (name, run) => platform.request(`${scope}:${name}`, run) };
}

function serve(count: number): FakeJmap {
  const server = new FakeJmap();
  server.putMailbox(mailbox("inbox", "inbox"));
  server.putMailbox(mailbox("archive", "archive"));
  for (let index = 1; index <= count; index += 1) {
    server.addEmail(email(`e${String(index)}`, { receivedAt: at(index) }));
  }
  vi.stubGlobal("fetch", server.fetch);
  return server;
}

function coordinate(posted: CacheMessage[]): Coordinator {
  return new Coordinator({
    store: store(),
    locks: locks(),
    post: (message) => {
      posted.push(message);
    },
  });
}

function posts(posted: CacheMessage[], count: number): () => void {
  return () => {
    expect(posted).toHaveLength(count);
  };
}

function requests(server: FakeJmap, count: number): () => void {
  return () => {
    expect(server.posted()).toHaveLength(count);
  };
}

// Waits for a poll or a forget to land, without moving the clock past
// the next poll.
function settled(check: () => void): Promise<void> {
  return vi.waitFor(check);
}

function drained(turns = DRAIN_TURNS): Promise<void> {
  if (turns === 0) {
    return Promise.resolve();
  }
  return new Promise((resolve) => {
    setImmediate(resolve);
  }).then(() => drained(turns - 1));
}

// Runs the given number of polls, each to its end.
async function polled(posted: CacheMessage[], count: number): Promise<void> {
  if (count === 0) {
    return;
  }
  const expected = posted.length + 1;
  await vi.advanceTimersByTimeAsync(CHANGES_POLL_MS);
  await settled(posts(posted, expected));
  await polled(posted, count - 1);
}

async function attached(count: number): Promise<Rig> {
  const server = serve(count);
  const posted: CacheMessage[] = [];
  const coordinator = coordinate(posted);
  const tab = coordinator.api();
  await tab.persisted();
  await tab.attach({ accounts: [ACCOUNT], listedAt: Date.now(), watching: null });
  await settled(posts(posted, 1));
  return { server, posted, coordinator, tab };
}

beforeEach(() => {
  scope = crypto.randomUUID();
  vi.useFakeTimers({ toFake: [...FAKED] });
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  indexedDB = new IDBFactory();
});

test("an attached account gets its mailbox tree at once and answers it from the store", async () => {
  const { server, posted, tab } = await attached(3);
  expect(server.posted()).toHaveLength(1);
  expect(posted).toEqual([
    { kind: "changed", accountId: ACCOUNT, mailboxes: true, windows: [], threads: [] },
  ]);
  const read = await tab.mailboxes(ACCOUNT);
  expect(read.ok && read.value.map((row) => row.id).toSorted()).toEqual(["archive", "inbox"]);
  expect(server.posted()).toHaveLength(1);
});

test("the poll follows the changes at its interval and names what moved", async () => {
  const { server, posted, tab } = await attached(3);
  await tab.window(ACCOUNT, "inbox", 0);
  await tab.attach({ accounts: [ACCOUNT], listedAt: Date.now(), watching: INBOX });
  server.addEmail(email("e4", { receivedAt: at(4) }));
  posted.length = 0;
  await vi.advanceTimersByTimeAsync(CHANGES_POLL_MS - BEFORE_POLL_MS);
  await drained();
  expect(posted).toEqual([]);
  await vi.advanceTimersByTimeAsync(BEFORE_POLL_MS);
  await settled(posts(posted, 1));
  expect(posted).toEqual([
    { kind: "changed", accountId: ACCOUNT, mailboxes: false, windows: ["inbox"], threads: [] },
  ]);
  const page = await tab.window(ACCOUNT, "inbox", 0);
  expect(page.ok && page.value.pending).toBe(1);
});

test("a list nobody watches is dropped by the poll while a watched one keeps its marker", async () => {
  const { server, posted, tab } = await attached(3);
  const second = coordinate([]).api();
  await second.persisted();
  await tab.window(ACCOUNT, "inbox", 0);
  await tab.window(ACCOUNT, "archive", 0);
  await second.attach({ accounts: [ACCOUNT], listedAt: Date.now(), watching: INBOX });
  server.addEmail(email("e4", { receivedAt: at(4) }));
  server.addEmail(email("a1", { mailboxIds: ["archive"], receivedAt: at(5) }));
  await vi.advanceTimersByTimeAsync(CHANGES_POLL_MS);
  await settled(posts(posted, 2));
  expect(await store().query(ACCOUNT, "archive")).toBeNull();
  expect((await store().query(ACCOUNT, "inbox"))?.pending).toEqual(["e4"]);
});

test("a watch the tab stops renewing lapses with the lease and its list is dropped", async () => {
  const { server, posted, tab } = await attached(3);
  await tab.window(ACCOUNT, "inbox", 0);
  await tab.attach({ accounts: [ACCOUNT], listedAt: Date.now(), watching: INBOX });
  server.addEmail(email("e4", { receivedAt: at(4) }));
  await polled(posted, Math.floor(LEASE_MS / CHANGES_POLL_MS));
  expect((await store().query(ACCOUNT, "inbox"))?.pending).toEqual(["e4"]);
  server.addEmail(email("e5", { receivedAt: at(5) }));
  await polled(posted, 1);
  expect(await store().query(ACCOUNT, "inbox")).toBeNull();
});

test("a fresh list drops the rows an earlier session left for another account", async () => {
  const { tab } = await attached(3);
  await tab.window(ACCOUNT, "inbox", 0);
  const posted: CacheMessage[] = [];
  const later = coordinate(posted).api();
  await later.persisted();
  await later.attach({ accounts: [], listedAt: Date.now(), watching: null });
  await settled(posts(posted, 1));
  expect(posted).toEqual([TREE_DROPPED]);
  expect(await store().mailboxes(ACCOUNT)).toEqual([]);
  expect(await store().query(ACCOUNT, "inbox")).toBeNull();
  expect(await store().state(ACCOUNT, "Email")).toBeNull();
});

test("a mailbox in its first sync brings the next poll closer until it is done", async () => {
  const server = serve(3);
  const inbox = { ...mailbox("inbox", "inbox"), totalEmails: 5, syncedEmails: 2 };
  server.putMailbox(inbox);
  const posted: CacheMessage[] = [];
  const tab = coordinate(posted).api();
  await tab.persisted();
  await tab.attach({ accounts: [ACCOUNT], listedAt: Date.now(), watching: null });
  await settled(posts(posted, 1));
  await vi.advanceTimersByTimeAsync(FIRST_SYNC_POLL_MS);
  await settled(posts(posted, 2));
  server.putMailbox({ ...inbox, syncedEmails: 5 });
  await vi.advanceTimersByTimeAsync(FIRST_SYNC_POLL_MS);
  await settled(posts(posted, 3));
  expect(posted[2]).toMatchObject({ mailboxes: true });
  await vi.advanceTimersByTimeAsync(FIRST_SYNC_POLL_MS);
  await drained();
  expect(posted).toHaveLength(3);
  await vi.advanceTimersByTimeAsync(CHANGES_POLL_MS - FIRST_SYNC_POLL_MS);
  await settled(posts(posted, 4));
});

test("a focus polls at once, but not twice inside the gap", async () => {
  const { server, posted, tab } = await attached(3);
  await vi.advanceTimersByTimeAsync(CHANGES_POLL_MS);
  await settled(posts(posted, 2));
  const before = server.posted().length;
  await tab.focus();
  await drained();
  expect(server.posted()).toHaveLength(before);
  await vi.advanceTimersByTimeAsync(PAST_FOCUS_GAP_MS);
  await tab.focus();
  await settled(posts(posted, 3));
  expect(server.posted()).toHaveLength(before + 1);
});

test("two workers on one store never interleave a window fetch", async () => {
  const server = serve(WINDOW_SIZE + 5);
  const first = coordinate([]).api();
  const second = coordinate([]).api();
  await first.persisted();
  await second.persisted();
  const [one, two] = await Promise.all([
    first.window(ACCOUNT, "inbox", 0),
    second.window(ACCOUNT, "inbox", 0),
  ]);
  expect(one.ok && one.value.rows).toHaveLength(WINDOW_SIZE);
  expect(two.ok && two.value.rows).toHaveLength(WINDOW_SIZE);
  expect(server.posted()).toHaveLength(1);
});

test("a write waits for the window's word on persistence", async () => {
  const server = serve(3);
  const tab = coordinate([]).api();
  let landed = false;
  const window = tab.window(ACCOUNT, "inbox", 0).then((page) => {
    landed = true;
    return page;
  });
  await settled(requests(server, 1));
  await drained();
  expect(landed).toBe(false);
  await tab.persisted();
  const page = await window;
  expect(page.ok && page.value.rows).toHaveLength(3);
});

test("a limit failure waits for the next poll and a lost session stops every account", async () => {
  const { server, posted, tab } = await attached(3);
  server.queue.push(json(400, LIMIT_PROBLEM, PROBLEM_TYPE));
  await vi.advanceTimersByTimeAsync(CHANGES_POLL_MS);
  await settled(requests(server, 2));
  await drained();
  expect(posted).toHaveLength(1);
  await vi.advanceTimersByTimeAsync(CHANGES_POLL_MS);
  await settled(posts(posted, 2));
  server.queue.push(json(401, { error: "unauthenticated" }));
  const listedBefore = Date.now();
  await vi.advanceTimersByTimeAsync(CHANGES_POLL_MS);
  await settled(posts(posted, 3));
  expect(posted[2]).toEqual(TREE_DROPPED);
  expect(await store().mailboxes(ACCOUNT)).toEqual([]);
  expect(await store().state(ACCOUNT, "Mailbox")).toBeNull();
  await vi.advanceTimersByTimeAsync(3 * CHANGES_POLL_MS);
  await drained();
  expect(server.posted()).toHaveLength(4);
  // A list from before the session ended starts nothing; one fetched after it does.
  await tab.attach({ accounts: [ACCOUNT], listedAt: listedBefore, watching: null });
  await drained();
  expect(server.posted()).toHaveLength(4);
  await tab.attach({ accounts: [ACCOUNT], listedAt: Date.now(), watching: null });
  await settled(requests(server, 5));
});

test("an older list never undoes a newer one", async () => {
  const { server, posted, coordinator } = await attached(3);
  const second = coordinator.api();
  await second.attach({ accounts: [], listedAt: Date.now() - CHANGES_POLL_MS, watching: null });
  await drained();
  expect(posted).toHaveLength(1);
  await vi.advanceTimersByTimeAsync(CHANGES_POLL_MS);
  await settled(posts(posted, 2));
  expect(server.posted()).toHaveLength(2);
  await second.attach({ accounts: [], listedAt: Date.now(), watching: null });
  await settled(posts(posted, 3));
  expect(posted[2]).toEqual(TREE_DROPPED);
});

test("a failure reaches the tab with its code and cause", async () => {
  const { server, tab } = await attached(3);
  server.queue.push(json(409, { error: "still_stopped", cause: "credentials" }));
  const page = await tab.window(ACCOUNT, "inbox", 0);
  expect(page).toEqual({
    ok: false,
    failure: { code: "stopped", stopCause: "credentials", limit: null },
  });
});

test("a poll that finds the account stopped tells every tab, and once more when it runs again", async () => {
  const { server, posted } = await attached(3);
  server.queue.push(json(409, { error: "still_stopped", cause: "connection" }));
  await vi.advanceTimersByTimeAsync(CHANGES_POLL_MS);
  await settled(posts(posted, 2));
  expect(posted[1]).toEqual({ kind: "account", accountId: ACCOUNT, stoppedCause: "connection" });
  server.queue.push(json(409, { error: "still_stopped", cause: "connection" }));
  await vi.advanceTimersByTimeAsync(CHANGES_POLL_MS);
  await settled(posts(posted, 3));
  await vi.advanceTimersByTimeAsync(CHANGES_POLL_MS);
  await settled(posts(posted, 5));
  expect(posted.slice(3)).toEqual([
    { kind: "changed", accountId: ACCOUNT, mailboxes: false, windows: [], threads: [] },
    { kind: "account", accountId: ACCOUNT, stoppedCause: null },
  ]);
  await vi.advanceTimersByTimeAsync(CHANGES_POLL_MS);
  await settled(posts(posted, 6));
  expect(posted[5]).toMatchObject({ kind: "changed" });
});

test("an account that left the list stops polling and leaves no rows", async () => {
  const { server, posted, tab } = await attached(3);
  await tab.window(ACCOUNT, "inbox", 0);
  posted.length = 0;
  await tab.attach({ accounts: [], listedAt: Date.now(), watching: null });
  await settled(posts(posted, 1));
  expect(posted).toEqual([TREE_DROPPED]);
  expect(await store().mailboxes(ACCOUNT)).toEqual([]);
  expect(await store().query(ACCOUNT, "inbox")).toBeNull();
  expect(await store().state(ACCOUNT, "Email")).toBeNull();
  await vi.advanceTimersByTimeAsync(2 * CHANGES_POLL_MS);
  await drained();
  expect(server.posted()).toHaveLength(2);
});

test("reveal lands the new mail and tells every tab; a thread reads from the store", async () => {
  const { server, posted, tab } = await attached(3);
  await tab.window(ACCOUNT, "inbox", 0);
  await tab.attach({ accounts: [ACCOUNT], listedAt: Date.now(), watching: INBOX });
  server.addEmail(email("e4", { receivedAt: at(4) }));
  await vi.advanceTimersByTimeAsync(CHANGES_POLL_MS);
  await settled(posts(posted, 2));
  posted.length = 0;
  await tab.reveal(ACCOUNT, "inbox");
  expect(posted).toEqual([
    { kind: "changed", accountId: ACCOUNT, mailboxes: false, windows: ["inbox"], threads: [] },
  ]);
  const page = await tab.window(ACCOUNT, "inbox", 0);
  expect(page.ok && page.value.rows[0]?.id).toBe("e4");
  const thread = await tab.thread(ACCOUNT, "t-e4");
  expect(thread.ok && thread.value?.emails["e4"]?.subject).toBe("Message e4");
  const unknown = await tab.thread(ACCOUNT, "t-e9");
  expect(unknown).toEqual({ ok: true, value: null });
});

test("work in flight when the database is cleared writes nothing after it", async () => {
  const { server, posted, tab } = await attached(3);
  const answer = Promise.withResolvers<undefined>();
  const held: typeof fetch = async (input, init) => {
    await answer.promise;
    return server.fetch(input, init);
  };
  vi.stubGlobal("fetch", held);
  server.addEmail(email("e4", { receivedAt: at(4) }));
  await vi.advanceTimersByTimeAsync(CHANGES_POLL_MS);
  const page = tab.window(ACCOUNT, "inbox", 0);
  await drained();
  posted.length = 0;
  await tab.clear("tab-1");
  answer.resolve(undefined);
  expect(await page).toEqual({
    ok: false,
    failure: { code: "unauthenticated", stopCause: null, limit: null },
  });
  await drained();
  expect(posted).toEqual([{ kind: "cleared", by: "tab-1" }]);
  expect(await store().accounts()).toEqual([]);
});

test("a write held for persistence lands nothing once the database was cleared", async () => {
  const server = serve(3);
  const posted: CacheMessage[] = [];
  const tab = coordinate(posted).api();
  await tab.attach({ accounts: [ACCOUNT], listedAt: Date.now(), watching: null });
  await settled(requests(server, 1));
  await drained();
  await tab.clear("tab-1");
  await tab.persisted();
  await drained();
  expect(posted).toEqual([{ kind: "cleared", by: "tab-1" }]);
  expect(await store().accounts()).toEqual([]);
});

test("clear stops the accounts, deletes the database and says so", async () => {
  const { server, posted, tab } = await attached(3);
  await tab.window(ACCOUNT, "inbox", 0);
  posted.length = 0;
  await tab.clear("tab-1");
  expect(posted).toEqual([{ kind: "cleared", by: "tab-1" }]);
  expect(await store().mailboxes(ACCOUNT)).toEqual([]);
  const sent = server.posted().length;
  // A renewal of a list fetched at or before the sign-out starts nothing.
  await tab.attach({ accounts: [ACCOUNT], listedAt: Date.now(), watching: null });
  await vi.advanceTimersByTimeAsync(2 * CHANGES_POLL_MS);
  await drained();
  expect(server.posted()).toHaveLength(sent);
});
