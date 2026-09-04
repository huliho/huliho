// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { relativeTime } from "./relative-time";

const NOW = new Date("2026-05-14T10:00:00Z");
const SECOND_MS = 1_000;
const MINUTE_MS = 60 * SECOND_MS;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

function ago(ms: number): Date {
  return new Date(NOW.getTime() - ms);
}

test("under a minute is null, as is any clock skew into the future", () => {
  expect(relativeTime("en", ago(59 * SECOND_MS), NOW)).toBeNull();
  expect(relativeTime("en", ago(-5 * SECOND_MS), NOW)).toBeNull();
});

test("minutes, hours and days pick the largest fitting unit", () => {
  expect(relativeTime("en", ago(2 * MINUTE_MS), NOW)).toBe("2 minutes ago");
  expect(relativeTime("en", ago(3 * HOUR_MS), NOW)).toBe("3 hours ago");
  expect(relativeTime("en", ago(DAY_MS), NOW)).toBe("yesterday");
  expect(relativeTime("en", ago(3 * DAY_MS), NOW)).toBe("3 days ago");
});

test("weeks, months and years follow", () => {
  expect(relativeTime("en", ago(21 * DAY_MS), NOW)).toBe("3 weeks ago");
  expect(relativeTime("en", ago(65 * DAY_MS), NOW)).toBe("2 months ago");
  expect(relativeTime("en", ago(400 * DAY_MS), NOW)).toBe("last year");
});

test("the locale shapes the words", () => {
  expect(relativeTime("nl", ago(2 * HOUR_MS), NOW)).toBe("2 uur geleden");
  expect(relativeTime("nl", ago(DAY_MS), NOW)).toBe("gisteren");
});
