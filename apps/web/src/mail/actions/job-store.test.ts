// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { JobProgress } from "@huliho/core";
import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import {
  JOB_LAPSE_MS,
  askedOffline,
  heardJob,
  onJobEnd,
  useAskedOffline,
  useRunningJob,
} from "./job-store";

const ACCOUNT = "a1";
const OTHER = "a2";

function progress(state: JobProgress["state"], done = 0): JobProgress {
  return { job: { kind: "markRead", mailboxId: "inbox" }, done, total: 9, last: null, state };
}

afterEach(() => {
  for (const accountId of [ACCOUNT, OTHER]) {
    heardJob(accountId, progress("done"));
  }
  window.dispatchEvent(new Event("online"));
  vi.useRealTimers();
});

test("a running job stands per account until its end, which every listener hears once", () => {
  const ended = vi.fn<(accountId: string, progress: JobProgress) => void>();
  const stop = onJobEnd(ended);
  const { result } = renderHook(() => [useRunningJob(ACCOUNT), useRunningJob(OTHER)]);
  act(() => {
    heardJob(ACCOUNT, progress("running", 2));
  });
  expect(result.current).toEqual([progress("running", 2), null]);
  act(() => {
    heardJob(ACCOUNT, progress("failed", 2));
  });
  expect(result.current).toEqual([null, null]);
  expect(ended).toHaveBeenCalledExactlyOnceWith(ACCOUNT, progress("failed", 2));
  stop();
});

test("a running job the worker falls silent on lapses, without an end", () => {
  vi.useFakeTimers();
  const ended = vi.fn<(accountId: string, progress: JobProgress) => void>();
  const stop = onJobEnd(ended);
  const { result } = renderHook(() => useRunningJob(ACCOUNT));
  act(() => {
    heardJob(ACCOUNT, progress("running"));
  });
  act(() => {
    vi.advanceTimersByTime(JOB_LAPSE_MS - 1);
    heardJob(ACCOUNT, progress("running", 2));
    vi.advanceTimersByTime(JOB_LAPSE_MS - 1);
  });
  expect(result.current?.done).toBe(2);
  act(() => {
    vi.advanceTimersByTime(1);
  });
  expect(result.current).toBeNull();
  expect(ended).not.toHaveBeenCalled();
  stop();
});

test("an ask made offline stands for its account until the connection is back or a job runs", () => {
  const { result } = renderHook(() => [useAskedOffline(ACCOUNT), useAskedOffline(OTHER)]);
  act(() => {
    askedOffline(ACCOUNT);
  });
  expect(result.current).toEqual([true, false]);
  act(() => {
    window.dispatchEvent(new Event("online"));
  });
  expect(result.current).toEqual([false, false]);
  act(() => {
    askedOffline(ACCOUNT);
    heardJob(ACCOUNT, progress("running"));
  });
  expect(result.current).toEqual([false, false]);
});
