// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { formatRowTime, startOfDay } from "./row-time";

const TODAY = startOfDay(new Date("2026-05-14T10:00"));

// A local wall-clock moment as the server's UTC string.
function local(moment: string): string {
  return new Date(moment).toISOString();
}

test("today shows the time, the six days before it the weekday", () => {
  expect(formatRowTime(local("2026-05-14T09:41"), TODAY, "en")).toBe("9:41 AM");
  expect(formatRowTime(local("2026-05-14T09:41"), TODAY, "nl")).toBe("09:41");
  expect(formatRowTime(local("2026-05-13T23:59"), TODAY, "en")).toBe("Wed");
  expect(formatRowTime(local("2026-05-08T08:00"), TODAY, "en")).toBe("Fri");
  expect(formatRowTime(local("2026-05-08T08:00"), TODAY, "nl")).toBe("vr");
});

test("older mail shows the day and month, with the year once it differs", () => {
  expect(formatRowTime(local("2026-05-07T08:00"), TODAY, "en")).toBe("May 7");
  expect(formatRowTime(local("2026-05-07T08:00"), TODAY, "nl")).toBe("7 mei");
  expect(formatRowTime(local("2025-10-14T08:00"), TODAY, "en")).toBe("Oct 14, 2025");
  expect(formatRowTime(local("2025-10-14T08:00"), TODAY, "nl")).toBe("14 okt 2025");
});

test("a time from the future still reads as a time", () => {
  expect(formatRowTime(local("2026-05-15T01:05"), TODAY, "en")).toBe("1:05 AM");
});
