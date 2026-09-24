// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The days before today a weekday still names on its own.
const RECENT_DAYS = 6;
const DAY_MS = 86_400_000;

type Shape = "time" | "weekday" | "date" | "dated" | "stamp";

const SHAPES = new Map<Shape, Intl.DateTimeFormatOptions>([
  ["time", { timeStyle: "short" }],
  ["weekday", { weekday: "short" }],
  ["date", { day: "numeric", month: "short" }],
  ["dated", { day: "numeric", month: "short", year: "numeric" }],
  ["stamp", { dateStyle: "medium", timeStyle: "short" }],
]);

// One formatter per locale and shape: building one costs more than a
// row may spend while the list scrolls.
const formatters = new Map<string, Intl.DateTimeFormat>();

function formatter(locale: string, shape: Shape): Intl.DateTimeFormat {
  const key = `${locale}/${shape}`;
  const held = formatters.get(key);
  if (held !== undefined) {
    return held;
  }
  const made = new Intl.DateTimeFormat(locale, SHAPES.get(shape));
  formatters.set(key, made);
  return made;
}

// The start of the local day `at` falls in.
export function startOfDay(at: Date): number {
  const day = new Date(at);
  day.setHours(0, 0, 0, 0);
  return day.getTime();
}

function shapeOf(at: Date, today: number): Shape {
  const daysAgo = Math.round((today - startOfDay(at)) / DAY_MS);
  if (daysAgo <= 0) {
    return "time";
  }
  if (daysAgo <= RECENT_DAYS) {
    return "weekday";
  }
  return new Date(today).getFullYear() === at.getFullYear() ? "date" : "dated";
}

// Today: the time. The six days before it: the weekday. Older: the day
// and month, with the year when it is not this one.
export function formatRowTime(receivedAt: string, today: number, locale: string): string {
  const at = new Date(receivedAt);
  return formatter(locale, shapeOf(at, today)).format(at);
}

// The whole moment, as an open message shows it.
export function formatMessageTime(receivedAt: string, locale: string): string {
  return formatter(locale, "stamp").format(new Date(receivedAt));
}
