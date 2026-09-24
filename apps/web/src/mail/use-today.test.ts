// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { startOfDay } from "./row-time";
import { useToday } from "./use-today";

const DAY_MS = 86_400_000;
const HALF_MINUTE_MS = 30_000;
// Half a minute before a midnight.
const LATE = new Date(2026, 4, 14, 23, 59, 30);
const NEXT_DAY = new Date(2026, 4, 15);
const DAY_AFTER = new Date(2026, 4, 16);

beforeEach(() => {
  vi.useFakeTimers({ now: LATE });
});

afterEach(() => {
  vi.useRealTimers();
});

test("the day turns at midnight and at the midnight after it", () => {
  const { result } = renderHook(() => useToday());
  expect(result.current).toBe(startOfDay(LATE));
  act(() => {
    vi.advanceTimersByTime(HALF_MINUTE_MS);
  });
  expect(result.current).toBe(NEXT_DAY.getTime());
  act(() => {
    vi.advanceTimersByTime(DAY_MS);
  });
  expect(result.current).toBe(DAY_AFTER.getTime());
});

test("a tab back in view reads the day afresh", () => {
  const { result } = renderHook(() => useToday());
  vi.setSystemTime(DAY_AFTER);
  expect(result.current).toBe(startOfDay(LATE));
  act(() => {
    document.dispatchEvent(new Event("visibilitychange"));
  });
  expect(result.current).toBe(DAY_AFTER.getTime());
});
