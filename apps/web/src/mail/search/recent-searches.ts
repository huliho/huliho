// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { RECENT_KEY_PREFIX, keptRecents } from "./recent-store";

// How many searches an account remembers; the next one pushes the oldest out.
export const RECENT_SEARCHES = 8;

function isKeyList(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((entry) => typeof entry === "string");
}

function stored(accountId: string): string[] {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(RECENT_KEY_PREFIX + accountId) ?? "[]");
    return isKeyList(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function dropStored(accountId: string): void {
  try {
    localStorage.removeItem(RECENT_KEY_PREFIX + accountId);
  } catch {
    // A storage that refuses to be read holds nothing to forget.
  }
}

// The account's recent searches, newest first, each a search key. Under
// `strict` they live in memory alone and the device holds none.
export function recentSearches(accountId: string, strict: boolean): string[] {
  if (strict) {
    dropStored(accountId);
    return keptRecents.get(accountId) ?? [];
  }
  return stored(accountId);
}

// A search that ran joins the front; a repeat moves up.
export function noteSearch(accountId: string, key: string, strict: boolean): void {
  const next = [key, ...recentSearches(accountId, strict).filter((entry) => entry !== key)].slice(
    0,
    RECENT_SEARCHES,
  );
  if (strict) {
    keptRecents.set(accountId, next);
    return;
  }
  try {
    localStorage.setItem(RECENT_KEY_PREFIX + accountId, JSON.stringify(next));
  } catch {
    // A device that keeps nothing simply starts without recents.
  }
}
