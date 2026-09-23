// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";

import { PANE_MIN_WIDTH_PX, useListWidth } from "./list-width";

const KEY = "huliho-list-width";
const CHOSEN_PX = 400;
const STORED_PX = 480;

afterEach(() => {
  cleanup();
  localStorage.clear();
});

test("a stored width that is not a whole number or below the floor reads as the default", () => {
  for (const held of ["abc", "12.5", String(PANE_MIN_WIDTH_PX - 1), ""]) {
    localStorage.setItem(KEY, held);
    expect(renderHook(() => useListWidth()).result.current[0]).toBeNull();
  }
  localStorage.setItem(KEY, String(STORED_PX));
  expect(renderHook(() => useListWidth()).result.current[0]).toBe(STORED_PX);
});

test("a chosen width is kept on the device and a reset removes it", () => {
  const { result } = renderHook(() => useListWidth());
  act(() => {
    result.current[1](CHOSEN_PX);
  });
  expect(result.current[0]).toBe(CHOSEN_PX);
  expect(localStorage.getItem(KEY)).toBe(String(CHOSEN_PX));
  act(() => {
    result.current[1](null);
  });
  expect(result.current[0]).toBeNull();
  expect(localStorage.getItem(KEY)).toBeNull();
});
