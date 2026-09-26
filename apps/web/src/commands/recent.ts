// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

const RECENT_KEY = "huliho-recent-commands";
// How many commands the palette remembers as recent, per device.
export const RECENT_LIMIT = 5;

function isIdList(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((entry) => typeof entry === "string");
}

// The ids of the commands last run from the palette, newest first.
export function recentCommands(): string[] {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    return isIdList(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

export function noteRecent(id: string): void {
  const next = [id, ...recentCommands().filter((entry) => entry !== id)].slice(0, RECENT_LIMIT);
  localStorage.setItem(RECENT_KEY, JSON.stringify(next));
}

// A session that ends in this browser takes the memory with it.
export function forgetRecent(): void {
  localStorage.removeItem(RECENT_KEY);
}
