// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test } from "vitest";

import { RECENT_LIMIT, forgetRecent, noteRecent, recentCommands } from "./recent";

afterEach(() => {
  localStorage.clear();
});

test("the last commands run come back newest first, each once, up to the limit", () => {
  for (let index = 0; index <= RECENT_LIMIT; index += 1) {
    noteRecent(`c${String(index)}`);
  }
  noteRecent("c3");
  const listed = recentCommands();
  expect(listed).toHaveLength(RECENT_LIMIT);
  expect(listed[0]).toBe("c3");
  expect(listed).not.toContain("c0");
  expect(new Set(listed).size).toBe(listed.length);
});

test("a session that ends forgets the commands last run", () => {
  noteRecent("c1");
  forgetRecent();
  expect(recentCommands()).toEqual([]);
  expect(localStorage.getItem("huliho-recent-commands")).toBeNull();
});

test("a memory that is not a list of ids reads as none", () => {
  localStorage.setItem("huliho-recent-commands", "{oops");
  expect(recentCommands()).toEqual([]);
  localStorage.setItem("huliho-recent-commands", JSON.stringify([1, 2]));
  expect(recentCommands()).toEqual([]);
});
