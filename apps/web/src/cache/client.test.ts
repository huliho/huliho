// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { queryKeys } from "@huliho/state";
import { QueryClient } from "@tanstack/react-query";
import { afterEach, expect, test, vi } from "vitest";

import { TAB, applyCacheMessage } from "./client";
import { LEASE_RENEW_MS } from "./coordinator";
import type { Lease } from "./coordinator";
import { CACHE_CHANNEL, readCacheMessage } from "./messages";

// The worker's remote as the window sees it, one fake per test.
const wrap = vi.hoisted(() => vi.fn<() => unknown>());
vi.mock("comlink", () => ({ wrap }));

const LEASE: Lease = { accounts: ["a1"], listedAt: 1, watching: null };

const NOTHING = { queryFn: () => Promise.resolve(1), staleTime: Number.POSITIVE_INFINITY };

async function client(): Promise<QueryClient> {
  const queryClient = new QueryClient();
  const keys = [
    queryKeys.mailboxes("a1"),
    queryKeys.window("a1", "inbox", 0),
    queryKeys.window("a1", "inbox", 1),
    queryKeys.window("a1", "sent", 0),
    queryKeys.thread("a1", "t1"),
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

test("a change invalidates the tree, every page of a moved list and the named threads", async () => {
  const queryClient = await client();
  const onCleared = vi.fn<() => void>();
  applyCacheMessage(
    queryClient,
    { kind: "changed", accountId: "a1", mailboxes: true, windows: ["inbox"], threads: ["t1"] },
    onCleared,
  );
  expect(stale(queryClient).toSorted()).toEqual([
    "a1/mailboxes",
    "a1/thread/t1",
    "a1/window/inbox/0",
    "a1/window/inbox/1",
  ]);
  expect(onCleared).not.toHaveBeenCalled();
});

test("a database another tab cleared takes every mail query, nothing else, then hands over", async () => {
  const queryClient = await client();
  const onCleared = vi.fn<() => void>();
  applyCacheMessage(queryClient, { kind: "cleared", by: "another tab" }, onCleared);
  expect(kept(queryClient)).toEqual(["accounts"]);
  expect(onCleared).toHaveBeenCalledOnce();
});

test("a database this tab cleared hands nothing over", async () => {
  const queryClient = await client();
  const onCleared = vi.fn<() => void>();
  applyCacheMessage(queryClient, { kind: "cleared", by: TAB }, onCleared);
  expect(kept(queryClient)).toEqual(["accounts"]);
  expect(onCleared).not.toHaveBeenCalled();
});

test("only a message of the worker's shape is read", () => {
  const changed = { kind: "changed", accountId: "a1", mailboxes: false, windows: [], threads: [] };
  expect(readCacheMessage(changed)).toEqual(changed);
  expect(readCacheMessage({ kind: "cleared", by: "t1" })).toEqual({ kind: "cleared", by: "t1" });
  expect(readCacheMessage({ kind: "cleared" })).toBeNull();
  expect(readCacheMessage({ ...changed, windows: [1] })).toBeNull();
  expect(readCacheMessage({ ...changed, accountId: 1 })).toBeNull();
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

test("the listener hears the worker's messages on the channel and nothing else", async () => {
  const { windowSide } = await page();
  const onCleared = vi.fn<() => void>();
  const stop = windowSide.installCacheListener(new QueryClient(), onCleared);
  const worker = new BroadcastChannel(CACHE_CHANNEL);
  const post = worker.postMessage.bind(worker);
  post({ kind: "other" });
  post({ kind: "cleared", by: "another tab" });
  await vi.waitFor(() => {
    expect(onCleared).toHaveBeenCalledOnce();
  });
  stop();
  post({ kind: "cleared", by: "another tab" });
  worker.close();
  expect(onCleared).toHaveBeenCalledOnce();
});

test("nothing is invalidated for an account the message does not name", async () => {
  const queryClient = await client();
  const spy = vi.spyOn(queryClient, "invalidateQueries");
  applyCacheMessage(
    queryClient,
    { kind: "changed", accountId: "a2", mailboxes: false, windows: [], threads: [] },
    () => undefined,
  );
  expect(spy).not.toHaveBeenCalled();
});
