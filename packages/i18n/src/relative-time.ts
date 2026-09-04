// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

const MS_PER_MINUTE = 60_000;
const MS_PER_HOUR = 60 * MS_PER_MINUTE;
const MS_PER_DAY = 24 * MS_PER_HOUR;
const MS_PER_WEEK = 7 * MS_PER_DAY;
// Calendar-free month and year; "3 months ago" needs no better.
const MS_PER_MONTH = 30 * MS_PER_DAY;
const MS_PER_YEAR = 365 * MS_PER_DAY;

// Largest first; the first unit that fits names the distance.
const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", MS_PER_YEAR],
  ["month", MS_PER_MONTH],
  ["week", MS_PER_WEEK],
  ["day", MS_PER_DAY],
  ["hour", MS_PER_HOUR],
  ["minute", MS_PER_MINUTE],
];

// How long ago `at` was. Under a minute the answer is null, so the
// caller can say "active now" in its own words.
export function relativeTime(locale: string, at: Date, now: Date): string | null {
  const elapsed = now.getTime() - at.getTime();
  const unit = UNITS.find(([, length]) => elapsed >= length);
  if (unit === undefined) {
    return null;
  }
  const [name, length] = unit;
  const format = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  return format.format(-Math.floor(elapsed / length), name);
}
