// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { RECENT_SEARCHES, noteSearch, recentSearches } from "./recent-searches";
import { RECENT_KEY_PREFIX, forgetRecentSearches } from "./recent-store";

afterEach(() => {
  forgetRecentSearches();
  localStorage.clear();
  vi.restoreAllMocks();
});

test("an account keeps its searches newest first, a repeat moving up", () => {
  noteSearch("acc-1", "a", false);
  noteSearch("acc-1", "b", false);
  noteSearch("acc-1", "a", false);
  expect(recentSearches("acc-1", false)).toEqual(["a", "b"]);
  expect(recentSearches("acc-2", false)).toEqual([]);
});

test("the next search past the cap pushes the oldest out", () => {
  const keys = Array.from({ length: RECENT_SEARCHES + 1 }, (_, index) => `k${String(index)}`);
  for (const key of keys) {
    noteSearch("acc-1", key, false);
  }
  expect(recentSearches("acc-1", false)).toEqual(keys.slice(1).toReversed());
});

test("under strict the searches live in memory and the device holds none", () => {
  noteSearch("acc-1", "stored", false);
  expect(recentSearches("acc-1", true)).toEqual([]);
  expect(localStorage.getItem(`${RECENT_KEY_PREFIX}acc-1`)).toBeNull();
  noteSearch("acc-1", "kept", true);
  expect(recentSearches("acc-1", true)).toEqual(["kept"]);
  expect(localStorage.length).toBe(0);
});

test("a sign-out forgets every account's searches and leaves the rest of the storage", () => {
  localStorage.setItem("huliho-list-width", "400");
  noteSearch("acc-1", "a", false);
  noteSearch("acc-2", "b", false);
  noteSearch("acc-3", "c", true);
  forgetRecentSearches();
  expect(recentSearches("acc-1", false)).toEqual([]);
  expect(recentSearches("acc-2", false)).toEqual([]);
  expect(recentSearches("acc-3", true)).toEqual([]);
  expect(localStorage.getItem("huliho-list-width")).toBe("400");
});

test("a stored value that is not a list of keys reads as none", () => {
  localStorage.setItem(`${RECENT_KEY_PREFIX}acc-1`, "{not json");
  expect(recentSearches("acc-1", false)).toEqual([]);
  localStorage.setItem(`${RECENT_KEY_PREFIX}acc-1`, JSON.stringify([1, "a"]));
  expect(recentSearches("acc-1", false)).toEqual([]);
});

test("a storage that refuses every call leaves the field without recents", () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new DOMException("denied", "SecurityError");
  });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new DOMException("denied", "SecurityError");
  });
  expect(() => {
    noteSearch("acc-1", "a", false);
  }).not.toThrow();
  expect(recentSearches("acc-1", false)).toEqual([]);
});
