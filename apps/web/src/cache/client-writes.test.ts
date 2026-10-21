// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// @vitest-environment node

import type { Mutation, Undo } from "@huliho/core";
import { queryKeys } from "@huliho/state";
import { QueryClient } from "@tanstack/react-query";
import { afterEach, expect, test, vi } from "vitest";

import { LEASE_LOCK } from "./locks";
import type { CacheResult } from "./outcome";

// The worker's remote as the window sees it, one fake per test.
const wrap = vi.hoisted(() => vi.fn<() => unknown>());
vi.mock("comlink", () => ({ wrap }));

const DESTROY: Mutation = { type: "Email", id: "e1", threadId: "t1", destroy: true };
const STAR: Mutation = {
  type: "Email",
  id: "e1",
  threadId: "t1",
  patch: { "keywords/$flagged": true },
};

function answered(held: number[]): CacheResult<Undo> {
  return { ok: true, value: { inverses: [], held, threads: ["t1"] } };
}

// The leases the origin holds now, by tab.
async function leases(): Promise<string[]> {
  const { held = [] } = await navigator.locks.query();
  return held.flatMap(({ name }) =>
    name?.startsWith(LEASE_LOCK) === true ? [name.slice(LEASE_LOCK.length)] : [],
  );
}

async function page(held: number[]) {
  vi.resetModules();
  const during: string[][] = [];
  const settle = vi.fn<() => Promise<CacheResult<void>>>(() =>
    Promise.resolve({ ok: true, value: undefined }),
  );
  const remote = {
    mutateMany: vi.fn<() => Promise<CacheResult<Undo>>>(async () => {
      during.push(await leases());
      return answered(held);
    }),
    release: settle,
    drop: settle,
  };
  wrap.mockReturnValue(remote);
  vi.stubGlobal("Worker", vi.fn<() => void>());
  return { remote, during, windowSide: await import("./client") };
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

test("a destroy holds this tab's lease from before the worker logs it until the release", async () => {
  const { remote, during, windowSide } = await page([1]);
  const { TAB, mailCache } = windowSide;
  const undo = await mailCache.mutateMany("a1", [DESTROY]);
  expect(during).toEqual([[TAB]]);
  expect(remote.mutateMany).toHaveBeenCalledExactlyOnceWith("a1", [DESTROY], TAB);
  expect(await leases()).toEqual([TAB]);
  await mailCache.release("a1", undo.held);
  await vi.waitFor(async () => {
    expect(await leases()).toEqual([]);
  });
});

test("the lease goes once the last held destroy is dropped, and a batch that held none lets it go at once", async () => {
  const { windowSide } = await page([1]);
  const { TAB, mailCache } = windowSide;
  await mailCache.mutateMany("a1", [DESTROY]);
  await mailCache.mutateMany("a1", [DESTROY]);
  await mailCache.drop("a1", [1]);
  expect(await leases()).toEqual([TAB]);
  await mailCache.release("a1", [1]);
  await vi.waitFor(async () => {
    expect(await leases()).toEqual([]);
  });
  const empty = await page([]);
  await empty.windowSide.mailCache.mutateMany("a1", [DESTROY]);
  await vi.waitFor(async () => {
    expect(await leases()).toEqual([]);
  });
});

test("a change without a destroy takes no lease", async () => {
  const { during, windowSide } = await page([]);
  await windowSide.mailCache.mutateMany("a1", [STAR]);
  expect(during).toEqual([[]]);
});

test("a tree that moved has the quota read again with it", async () => {
  const { windowSide } = await page([]);
  const queryClient = new QueryClient();
  const quiet = { queryFn: () => Promise.resolve(1), staleTime: Number.POSITIVE_INFINITY };
  await queryClient.query({ queryKey: queryKeys.quota("a1"), ...quiet });
  await queryClient.query({ queryKey: queryKeys.quota("a2"), ...quiet });
  const changed = { accountId: "a1", windows: [], threads: [] };
  const listeners = { cleared: () => undefined, refused: () => undefined, job: () => undefined };
  windowSide.applyCacheMessage(
    queryClient,
    { kind: "changed", mailboxes: false, ...changed },
    listeners,
  );
  expect(queryClient.getQueryState(queryKeys.quota("a1"))?.isInvalidated).toBe(false);
  windowSide.applyCacheMessage(
    queryClient,
    { kind: "changed", mailboxes: true, ...changed },
    listeners,
  );
  expect(queryClient.getQueryState(queryKeys.quota("a1"))?.isInvalidated).toBe(true);
  expect(queryClient.getQueryState(queryKeys.quota("a2"))?.isInvalidated).toBe(false);
});
