// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { queryKeys } from "@huliho/state";
import { QueryClient } from "@tanstack/react-query";
import { afterEach, expect, test, vi } from "vitest";

import { TAB, applyCacheMessage } from "./client";
import type { CacheListeners } from "./client";
import { LEASE_RENEW_MS } from "./coordinator";
import type { Lease } from "./coordinator";
import { CACHE_CHANNEL, readCacheMessage } from "./messages";
import type { CacheResult } from "./outcome";

// The worker's remote as the window sees it, one fake per test.
const wrap = vi.hoisted(() => vi.fn<() => unknown>());
vi.mock("comlink", () => ({ wrap }));

const LEASE: Lease = { accounts: ["a1"], listedAt: 1, watching: null, strict: false };

const NOTHING = { queryFn: () => Promise.resolve(1), staleTime: Number.POSITIVE_INFINITY };

const MAIL_OF_A1 = [
  "a1/body/e1",
  "a1/mailboxes",
  "a1/thread/t1",
  "a1/window/inbox/0",
  "a1/window/inbox/1",
  "a1/window/sent/0",
];

async function client(): Promise<QueryClient> {
  const queryClient = new QueryClient();
  const keys = [
    queryKeys.mailboxes("a1"),
    queryKeys.window("a1", "inbox", 0),
    queryKeys.window("a1", "inbox", 1),
    queryKeys.window("a1", "sent", 0),
    queryKeys.thread("a1", "t1"),
    queryKeys.body("a1", "e1"),
    queryKeys.mailboxes("a2"),
    queryKeys.accounts,
  ];
  await Promise.all(keys.map((queryKey) => queryClient.query({ queryKey, ...NOTHING })));
  return queryClient;
}

function stale(queryClient: QueryClient): string[] {
  return queryClient
    .getQueryCache()
    .findAll({ stale: true })
    .map((query) => query.queryKey.join("/"));
}

function kept(queryClient: QueryClient): string[] {
  return queryClient
    .getQueryCache()
    .getAll()
    .map((query) => query.queryKey.join("/"));
}

function listeners() {
  return { cleared: vi.fn<() => void>(), refused: vi.fn<() => void>() } satisfies CacheListeners;
}

test("a change invalidates the tree, every page of a moved list and the named threads", async () => {
  const queryClient = await client();
  const heard = listeners();
  applyCacheMessage(
    queryClient,
    { kind: "changed", accountId: "a1", mailboxes: true, windows: ["inbox"], threads: ["t1"] },
    heard,
  );
  expect(stale(queryClient).toSorted()).toEqual([
    "a1/mailboxes",
    "a1/thread/t1",
    "a1/window/inbox/0",
    "a1/window/inbox/1",
  ]);
  expect(heard.cleared).not.toHaveBeenCalled();
});

test("a database another tab cleared takes every mail query, nothing else, then hands over", async () => {
  const queryClient = await client();
  const heard = listeners();
  applyCacheMessage(queryClient, { kind: "cleared", by: "another tab" }, heard);
  expect(kept(queryClient)).toEqual(["accounts"]);
  expect(heard.cleared).toHaveBeenCalledOnce();
});

test("a database this tab cleared hands nothing over", async () => {
  const queryClient = await client();
  const heard = listeners();
  applyCacheMessage(queryClient, { kind: "cleared", by: TAB }, heard);
  expect(kept(queryClient)).toEqual(["accounts"]);
  expect(heard.cleared).not.toHaveBeenCalled();
});

test("an account the server stopped refetches the accounts list and nothing else", async () => {
  const queryClient = await client();
  const heard = listeners();
  applyCacheMessage(
    queryClient,
    { kind: "account", accountId: "a1", stoppedCause: "connection" },
    heard,
  );
  expect(stale(queryClient)).toEqual(["accounts"]);
  expect(heard.cleared).not.toHaveBeenCalled();
});

test("a refused change is the tab's to tell and touches no query", async () => {
  const queryClient = await client();
  const heard = listeners();
  applyCacheMessage(queryClient, { kind: "refused" }, heard);
  expect(heard.refused).toHaveBeenCalledOnce();
  expect(heard.cleared).not.toHaveBeenCalled();
  expect(stale(queryClient)).toEqual([]);
});

test("a worker on another store has every mail query read again and nothing else", async () => {
  const queryClient = await client();
  const heard = listeners();
  applyCacheMessage(queryClient, { kind: "reset" }, heard);
  expect(stale(queryClient).toSorted()).toEqual([...MAIL_OF_A1, "a2/mailboxes"]);
  expect(kept(queryClient)).toContain("accounts");
  expect(heard.cleared).not.toHaveBeenCalled();
});

test.each([
  ["says the instance stores mail", false, ["session"]],
  ["says the same", true, []],
])(
  "a worker that keeps mail off the disk has a tab whose session answer %s read it again or not",
  async (_what, privacyStrict, reread) => {
    const queryClient = await client();
    applyCacheMessage(queryClient, { kind: "strict" }, listeners());
    expect(stale(queryClient)).toEqual([]);
    queryClient.setQueryData(queryKeys.session, { privacyStrict });
    applyCacheMessage(queryClient, { kind: "strict" }, listeners());
    expect(stale(queryClient)).toEqual(reread);
  },
);

test("only a message of the worker's shape is read", () => {
  const changed = { kind: "changed", accountId: "a1", mailboxes: false, windows: [], threads: [] };
  expect(readCacheMessage(changed)).toEqual(changed);
  expect(readCacheMessage({ kind: "cleared", by: "t1" })).toEqual({ kind: "cleared", by: "t1" });
  expect(readCacheMessage({ kind: "cleared" })).toBeNull();
  const stopped = { kind: "account", accountId: "a1", stoppedCause: "credentials" };
  expect(readCacheMessage(stopped)).toEqual(stopped);
  expect(readCacheMessage({ ...stopped, stoppedCause: null })).toEqual({
    ...stopped,
    stoppedCause: null,
  });
  expect(readCacheMessage({ ...stopped, stoppedCause: "other" })).toBeNull();
  expect(readCacheMessage({ kind: "account", stoppedCause: null })).toBeNull();
  expect(readCacheMessage({ ...changed, windows: [1] })).toBeNull();
  expect(readCacheMessage({ ...changed, accountId: 1 })).toBeNull();
  expect(readCacheMessage({ kind: "refused", extra: true })).toEqual({ kind: "refused" });
  expect(readCacheMessage({ kind: "reset", extra: true })).toEqual({ kind: "reset" });
  expect(readCacheMessage({ kind: "strict", extra: true })).toEqual({ kind: "strict" });
  expect(readCacheMessage({ kind: "other" })).toBeNull();
  expect(readCacheMessage("changed")).toBeNull();
  expect(readCacheMessage(null)).toBeNull();
});

function fakeRemote() {
  return {
    attach: vi.fn<(lease: Lease) => Promise<void>>(() => Promise.resolve()),
    focus: vi.fn<() => Promise<void>>(() => Promise.resolve()),
    persisted: vi.fn<() => Promise<void>>(() => Promise.resolve()),
    clear: vi.fn<(by: string) => Promise<void>>(() => Promise.resolve()),
    mailboxes: vi.fn<(accountId: string) => Promise<CacheResult<unknown[]>>>(),
    body: vi.fn<(...call: unknown[]) => Promise<CacheResult<unknown>>>(),
    mutate: vi.fn<(...call: unknown[]) => Promise<CacheResult<void>>>(),
  };
}

// A fresh page: the module's worker and its one persistence request start over.
async function page(persist?: () => Promise<boolean>) {
  vi.resetModules();
  const remote = fakeRemote();
  wrap.mockReturnValue(remote);
  const spawn = vi.fn<() => void>();
  vi.stubGlobal("Worker", spawn);
  if (persist !== undefined) {
    Object.defineProperty(navigator, "storage", { configurable: true, value: { persist } });
  }
  return { remote, spawn, windowSide: await import("./client") };
}

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  Reflect.deleteProperty(navigator, "storage");
});

test("a tab leases on mount, renews at the interval, polls on its return and stops on cleanup", async () => {
  vi.useFakeTimers();
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  const { remote, spawn, windowSide } = await page();
  const detach = windowSide.attachCache(LEASE);
  expect(remote.attach).toHaveBeenCalledExactlyOnceWith(LEASE);
  await vi.advanceTimersByTimeAsync(LEASE_RENEW_MS);
  expect(remote.attach).toHaveBeenCalledTimes(2);
  document.dispatchEvent(new Event("visibilitychange"));
  expect(remote.attach).toHaveBeenCalledTimes(3);
  expect(remote.focus).toHaveBeenCalledOnce();
  detach();
  await vi.advanceTimersByTimeAsync(LEASE_RENEW_MS);
  document.dispatchEvent(new Event("visibilitychange"));
  expect(remote.attach).toHaveBeenCalledTimes(3);
  expect(spawn).toHaveBeenCalledOnce();
});

test("persistence is asked for once per page and reported even when the request fails", async () => {
  const persist = vi.fn<() => Promise<boolean>>(() => Promise.reject(new Error("refused")));
  const logged = vi.spyOn(console, "error").mockImplementation(() => undefined);
  const { remote, windowSide } = await page(persist);
  windowSide.attachCache(LEASE)();
  windowSide.attachCache(LEASE)();
  await vi.waitFor(() => {
    expect(remote.persisted).toHaveBeenCalledOnce();
  });
  expect(persist).toHaveBeenCalledOnce();
  expect(logged).toHaveBeenCalledOnce();
});

test("a strict instance asks for no persistent storage and still reports", async () => {
  const persist = vi.fn<() => Promise<boolean>>(() => Promise.resolve(true));
  const { remote, windowSide } = await page(persist);
  windowSide.attachCache({ ...LEASE, strict: true })();
  await vi.waitFor(() => {
    expect(remote.persisted).toHaveBeenCalledOnce();
  });
  expect(persist).not.toHaveBeenCalled();
  expect(remote.attach).toHaveBeenCalledExactlyOnceWith({ ...LEASE, strict: true });
});

test("a sign-out names this tab and survives a worker that fails or never answers", async () => {
  const logged = vi.spyOn(console, "error").mockImplementation(() => undefined);
  const { remote, windowSide } = await page();
  remote.clear.mockRejectedValueOnce(new Error("blocked"));
  await windowSide.clearCache();
  expect(remote.clear).toHaveBeenCalledWith(windowSide.TAB);
  expect(logged).toHaveBeenCalledOnce();
  vi.useFakeTimers();
  remote.clear.mockReturnValueOnce(new Promise(() => undefined));
  const cleared = windowSide.clearCache();
  await vi.advanceTimersByTimeAsync(windowSide.CLEAR_WAIT_MS);
  await expect(cleared).resolves.toBeUndefined();
});

test("a read hands the worker's value through and turns its failure back into the error", async () => {
  const { remote, windowSide } = await page();
  remote.mailboxes.mockResolvedValueOnce({ ok: true, value: [] });
  await expect(windowSide.mailCache.mailboxes("a1")).resolves.toEqual([]);
  remote.mailboxes.mockResolvedValueOnce({
    ok: false,
    failure: { code: "stopped", stopCause: "credentials", limit: null },
  });
  const failed = await windowSide.mailCache.mailboxes("a1").catch((error: unknown) => error);
  expect(failed).toBeInstanceOf(Error);
  expect(failed).toMatchObject({
    name: "JmapError",
    code: "stopped",
    stopCause: "credentials",
    limit: null,
  });
  expect(remote.mailboxes).toHaveBeenCalledWith("a1");
});

test("a body and a change cross to the worker with what the caller named", async () => {
  const { remote, windowSide } = await page();
  remote.body.mockResolvedValueOnce({ ok: true, value: null });
  await expect(windowSide.mailCache.body("a1", "e1", { large: true })).resolves.toBeNull();
  expect(remote.body).toHaveBeenCalledExactlyOnceWith("a1", "e1", { large: true });
  const mutation = { type: "Email" as const, id: "e1", patch: { "keywords/$seen": true as const } };
  remote.mutate.mockResolvedValueOnce({ ok: true, value: undefined });
  await expect(windowSide.mailCache.mutate("a1", mutation)).resolves.toBeUndefined();
  expect(remote.mutate).toHaveBeenCalledExactlyOnceWith("a1", mutation);
  remote.mutate.mockResolvedValueOnce({
    ok: false,
    failure: { code: "unavailable", stopCause: null, limit: null },
  });
  await expect(windowSide.mailCache.mutate("a1", mutation)).rejects.toMatchObject({
    code: "unavailable",
  });
});

test("the listener hears the worker's messages on the channel and nothing else", async () => {
  const { windowSide } = await page();
  const heard = listeners();
  const stop = windowSide.installCacheListener(new QueryClient(), heard);
  const worker = new BroadcastChannel(CACHE_CHANNEL);
  const post = worker.postMessage.bind(worker);
  post({ kind: "other" });
  post({ kind: "cleared", by: "another tab" });
  post({ kind: "refused" });
  await vi.waitFor(() => {
    expect(heard.refused).toHaveBeenCalledOnce();
  });
  expect(heard.cleared).toHaveBeenCalledOnce();
  stop();
  post({ kind: "cleared", by: "another tab" });
  worker.close();
  expect(heard.cleared).toHaveBeenCalledOnce();
});

test("nothing is invalidated for an account the message does not name", async () => {
  const queryClient = await client();
  const spy = vi.spyOn(queryClient, "invalidateQueries");
  applyCacheMessage(
    queryClient,
    { kind: "changed", accountId: "a2", mailboxes: false, windows: [], threads: [] },
    listeners(),
  );
  expect(spy).not.toHaveBeenCalled();
});
