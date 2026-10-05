// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// @vitest-environment node

import { MemoryMailStore } from "@huliho/core";
import type { Mutation } from "@huliho/core";
import { ACCOUNT, FakeJmap, UPSTREAM, at, email, json, mailbox } from "@huliho/core/testing";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { CHANGES_POLL_MS, Coordinator, FIRST_SYNC_POLL_MS } from "./coordinator";
import type { CacheApi } from "./coordinator";
import { webLocks } from "./locks";
import type { Locks } from "./locks";
import type { CacheMessage } from "./messages";
import { ChosenStore } from "./store-choice";
import { SET_FLUSH_MS } from "./writes";

const INBOX = { accountId: ACCOUNT, mailboxId: "inbox" };
const SEEN = "keywords/$seen";
const TEMPLATE = "/api/jmap/acc-1/download/{accountId}/{blobId}/{name}?type={type}";

// Only the clock is faked; the fake server answers on real promises.
const FAKED = ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"] as const;

// How many turns of the event loop let the work of a call finish.
const DRAIN_TURNS = 50;

// A step of the clock short of the flush window.
const INSIDE_WINDOW_MS = 100;

const ROWS_MOVED = {
  kind: "changed",
  accountId: ACCOUNT,
  mailboxes: true,
  windows: ["inbox"],
  threads: ["t1"],
};

interface Rig {
  server: FakeJmap;
  posted: CacheMessage[];
  coordinator: Coordinator;
  tab: CacheApi;
  store: ChosenStore;
  locks: Locks;
  erase: ReturnType<typeof vi.fn<() => Promise<void>>>;
}

let scope = "";

function read(id: string): Mutation {
  return { type: "Email", id, patch: { [SEEN]: true } };
}

function drained(turns = DRAIN_TURNS): Promise<void> {
  if (turns === 0) {
    return Promise.resolve();
  }
  return new Promise((resolve) => {
    setImmediate(resolve);
  }).then(() => drained(turns - 1));
}

// Runs the poll that is due now to its end.
async function polled(): Promise<void> {
  await vi.advanceTimersByTimeAsync(0);
  await drained();
}

function lease(strict = false) {
  return { accounts: [ACCOUNT], listedAt: Date.now(), watching: INBOX, strict };
}

// A thread of two unread emails and a third unread one in the inbox;
// the tree, the first page and the thread are held and the count of
// requests and posts starts over.
async function opened(knobs: (server: FakeJmap) => void = () => undefined): Promise<Rig> {
  const server = new FakeJmap();
  server.putMailbox(mailbox("inbox", "inbox"));
  server.addEmail(email("e1", { threadId: "t1", receivedAt: at(1) }));
  server.addEmail(email("e2", { threadId: "t1", receivedAt: at(2) }));
  server.addEmail(email("e3", { receivedAt: at(3) }));
  server.recount();
  knobs(server);
  vi.stubGlobal("fetch", server.fetch);
  const posted: CacheMessage[] = [];
  const erase = vi.fn<() => Promise<void>>(() => Promise.resolve());
  const store = new ChosenStore({ open: () => new MemoryMailStore(), erase });
  const platform = webLocks(navigator.locks);
  const locks: Locks = { request: (name, run) => platform.request(`${scope}:${name}`, run) };
  const coordinator = new Coordinator({
    store,
    choose: (strict) => store.choose(strict),
    locks,
    post: (message) => {
      posted.push(message);
    },
  });
  const tab = coordinator.api();
  await tab.persisted();
  await tab.attach(lease());
  await polled();
  await tab.window(ACCOUNT, "inbox", 0);
  await tab.thread(ACCOUNT, "t1");
  posted.length = 0;
  server.requests.length = 0;
  return { server, posted, coordinator, tab, store, locks, erase };
}

function idsOf(args: Record<string, unknown>): string[] {
  const { update } = args;
  return typeof update === "object" && update !== null ? Object.keys(update) : [];
}

// The email ids each Email/set sent so far named.
function sets(server: FakeJmap): string[][] {
  return server
    .posted()
    .flat()
    .filter(([name]) => name === "Email/set")
    .map(([, args]) => idsOf(args));
}

function methods(server: FakeJmap): string[] {
  return server
    .posted()
    .flat()
    .map(([name]) => name);
}

async function seen(store: ChosenStore, id: string): Promise<boolean> {
  return (await store.emails(ACCOUNT, [id])).get(id)?.keywords["$seen"] === true;
}

beforeEach(() => {
  scope = crypto.randomUUID();
  vi.useFakeTimers({ toFake: [...FAKED] });
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

test("a change lands in the rows at once, tells the tabs and goes out as one request after the window", async () => {
  const { server, posted, tab, store } = await opened();
  expect(await tab.mutate(ACCOUNT, read("e1"))).toEqual({ ok: true, value: undefined });
  await vi.advanceTimersByTimeAsync(INSIDE_WINDOW_MS);
  await tab.mutate(ACCOUNT, read("e2"));
  expect(posted).toEqual([ROWS_MOVED, ROWS_MOVED]);
  expect(await seen(store, "e1")).toBe(true);
  expect(await store.pending(ACCOUNT)).toHaveLength(2);
  expect(server.posted()).toHaveLength(0);
  await vi.advanceTimersByTimeAsync(SET_FLUSH_MS - INSIDE_WINDOW_MS);
  await drained();
  expect(sets(server)).toEqual([["e1", "e2"]]);
  expect(await store.pending(ACCOUNT)).toEqual([]);
  expect(server.emails.get("e2")?.keywords).toEqual({ $seen: true });
  expect(posted).toHaveLength(2);
});

test("a change that moves nothing tells nobody and sends nothing", async () => {
  const { server, posted, tab } = await opened();
  await tab.mutate(ACCOUNT, { type: "Email", id: "e1", patch: { [SEEN]: null } });
  await tab.mutate(ACCOUNT, read("gone"));
  await vi.advanceTimersByTimeAsync(SET_FLUSH_MS);
  await drained();
  expect(posted).toEqual([]);
  expect(server.posted()).toHaveLength(0);
});

test("a refused change gives the rows back and tells every tab of the refusal", async () => {
  const { server, posted, tab, store } = await opened((fake) => {
    fake.refused.set("e1", "forbidden");
  });
  await tab.mutate(ACCOUNT, read("e1"));
  await tab.mutate(ACCOUNT, read("e3"));
  posted.length = 0;
  await vi.advanceTimersByTimeAsync(SET_FLUSH_MS);
  await drained();
  expect(sets(server)).toEqual([["e1", "e3"]]);
  expect(posted).toEqual([ROWS_MOVED, { kind: "refused" }]);
  expect(await seen(store, "e1")).toBe(false);
  expect(await seen(store, "e3")).toBe(true);
  expect(await store.pending(ACCOUNT)).toEqual([]);
});

test("a server out of reach leaves the log for the next poll, which sends it ahead of the changes", async () => {
  const { server, posted, tab, store } = await opened();
  await tab.mutate(ACCOUNT, read("e1"));
  server.queue.push(json(502, { error: "upstream_unreachable" }));
  await vi.advanceTimersByTimeAsync(SET_FLUSH_MS);
  await drained();
  expect(server.posted()).toHaveLength(1);
  expect(await store.pending(ACCOUNT)).toHaveLength(1);
  expect(await seen(store, "e1")).toBe(true);
  expect(posted).toEqual([ROWS_MOVED]);
  server.requests.length = 0;
  await vi.advanceTimersByTimeAsync(CHANGES_POLL_MS);
  await drained();
  expect(methods(server)[0]).toBe("Email/set");
  expect(methods(server)).toContain("Email/changes");
  expect(await store.pending(ACCOUNT)).toEqual([]);
  expect(await seen(store, "e1")).toBe(true);
});

test("a flush that fails off the wire holds the poll up no more than a server out of reach", async () => {
  const logged = vi.spyOn(console, "error").mockImplementation(() => undefined);
  const { server, tab, store } = await opened();
  await tab.mutate(ACCOUNT, read("e1"));
  const broken = { methodResponses: [["Email/set", { updated: 1 }, "s"]], sessionState: "s1" };
  server.queue.push(json(200, broken));
  server.queue.push(json(200, broken));
  await vi.advanceTimersByTimeAsync(SET_FLUSH_MS);
  await drained();
  server.requests.length = 0;
  // The fixture's mailbox is in its first sync, so one poll is this far off.
  await vi.advanceTimersByTimeAsync(FIRST_SYNC_POLL_MS);
  await drained();
  expect(methods(server)).toContain("Email/changes");
  expect(await store.pending(ACCOUNT)).toHaveLength(1);
  expect(logged).toHaveBeenCalled();
  logged.mockRestore();
});

test("the network coming back polls at once, the log first", async () => {
  const { server, coordinator, tab, store } = await opened();
  await tab.mutate(ACCOUNT, read("e1"));
  server.queue.push(json(502, { error: "upstream_unreachable" }));
  await vi.advanceTimersByTimeAsync(SET_FLUSH_MS);
  await drained();
  server.requests.length = 0;
  coordinator.online();
  await polled();
  expect(methods(server)[0]).toBe("Email/set");
  expect(methods(server)).toContain("Email/changes");
  expect(await store.pending(ACCOUNT)).toEqual([]);
});

test("a log longer than one set takes goes out in rounds", async () => {
  const { server, tab, store } = await opened((fake) => {
    fake.maxObjectsInSet = 1;
  });
  await tab.mutate(ACCOUNT, read("e1"));
  await tab.mutate(ACCOUNT, read("e2"));
  await tab.mutate(ACCOUNT, read("e3"));
  await vi.advanceTimersByTimeAsync(SET_FLUSH_MS);
  await drained();
  expect(sets(server)).toEqual([["e1"], ["e2"], ["e3"]]);
  expect(await store.pending(ACCOUNT)).toEqual([]);
});

test("a read-only account and a mailbox without the right log nothing and send nothing", async () => {
  const readOnly = await opened((fake) => {
    fake.readOnly = true;
  });
  expect(await readOnly.tab.mutate(ACCOUNT, read("e1"))).toEqual({ ok: true, value: undefined });
  await vi.advanceTimersByTimeAsync(SET_FLUSH_MS);
  await drained();
  expect(readOnly.posted).toEqual([]);
  expect(await readOnly.store.pending(ACCOUNT)).toEqual([]);
  expect(await seen(readOnly.store, "e1")).toBe(false);
  expect(readOnly.server.posted()).toHaveLength(0);
  vi.unstubAllGlobals();
  const noRight = await opened((fake) => {
    const inbox = mailbox("inbox", "inbox");
    fake.putMailbox({ ...inbox, myRights: { ...inbox.myRights, maySetSeen: false } });
    fake.recount();
  });
  await noRight.tab.mutate(ACCOUNT, read("e1"));
  await vi.advanceTimersByTimeAsync(SET_FLUSH_MS);
  await drained();
  expect(noRight.posted).toEqual([]);
  expect(noRight.server.posted()).toHaveLength(0);
});

test("a sign-out drops the flush that waited and writes nothing after it", async () => {
  const { server, coordinator, tab } = await opened();
  await tab.mutate(ACCOUNT, read("e1"));
  await coordinator.api().clear("tab");
  await vi.advanceTimersByTimeAsync(SET_FLUSH_MS);
  await drained();
  expect(sets(server)).toEqual([]);
});

test("a body answers with where its parts download from and asks the server once", async () => {
  const { server, tab } = await opened();
  const first = await tab.body(ACCOUNT, "e1", { large: false });
  expect(first).toMatchObject({
    ok: true,
    value: {
      body: { id: "e1", large: false },
      download: { template: TEMPLATE, accountId: UPSTREAM },
    },
  });
  expect(server.posted()).toHaveLength(1);
  const again = await tab.body(ACCOUNT, "e1", { large: false });
  expect(again).toEqual(first);
  expect(server.posted()).toHaveLength(1);
  expect(await tab.body(ACCOUNT, "gone", { large: false })).toEqual({ ok: true, value: null });
});

test("a body waits for no poll and no other holder of the account's lock", async () => {
  const { tab, locks } = await opened();
  const release = Promise.withResolvers<undefined>();
  const held = locks.request(`huliho-cache:${ACCOUNT}`, () => release.promise);
  await drained();
  const body = await tab.body(ACCOUNT, "e1", { large: false });
  expect(body.ok && body.value?.body.id).toBe("e1");
  release.resolve(undefined);
  await held;
});

test("a body request the server cannot answer reaches the tab as unavailable", async () => {
  const { server, tab } = await opened();
  const down = { methodResponses: [["error", { type: "serverUnavailable" }, "b"]] };
  server.queue.push(json(200, { ...down, sessionState: "s1" }));
  expect(await tab.body(ACCOUNT, "e1", { large: false })).toEqual({
    ok: false,
    failure: { code: "unavailable", stopCause: null, limit: null },
  });
});

test("a tab that names the strict setting moves the worker to an empty store, which a tab with an older answer leaves alone", async () => {
  const { server, posted, coordinator, tab, store, erase } = await opened();
  await tab.mutate(ACCOUNT, read("e1"));
  posted.length = 0;
  await tab.attach(lease(true));
  await polled();
  expect(erase).toHaveBeenCalledOnce();
  expect(posted.slice(0, 2)).toEqual([{ kind: "reset" }, { kind: "strict" }]);
  expect(posted).toContainEqual({
    kind: "changed",
    accountId: ACCOUNT,
    mailboxes: true,
    windows: [],
    threads: [],
  });
  expect(await store.pending(ACCOUNT)).toEqual([]);
  expect((await store.emails(ACCOUNT, ["e1"])).size).toBe(0);
  expect(await store.mailboxes(ACCOUNT)).toHaveLength(1);
  server.requests.length = 0;
  await vi.advanceTimersByTimeAsync(SET_FLUSH_MS);
  await drained();
  expect(sets(server)).toEqual([]);
  await tab.attach(lease(true));
  await drained();
  expect(erase).toHaveBeenCalledOnce();
  posted.length = 0;
  await coordinator.api().attach(lease(false));
  await polled();
  expect(posted).not.toContainEqual({ kind: "reset" });
  expect(posted).not.toContainEqual({ kind: "strict" });
  expect(await store.mailboxes(ACCOUNT)).toHaveLength(1);
  expect(erase).toHaveBeenCalledOnce();
});

test.each([
  [true, [{ kind: "strict" }]],
  [false, []],
])(
  "a worker whose first tab names strict as %j tells the tabs or stays silent",
  async (strict, told) => {
    const posted: CacheMessage[] = [];
    const store = new ChosenStore({
      open: () => new MemoryMailStore(),
      erase: () => Promise.resolve(),
    });
    const coordinator = new Coordinator({
      store,
      choose: (named) => store.choose(named),
      locks: webLocks(navigator.locks),
      post: (message) => {
        posted.push(message);
      },
    });
    await coordinator.api().attach({ accounts: [], listedAt: 0, watching: null, strict });
    expect(posted).toEqual(told);
  },
);
