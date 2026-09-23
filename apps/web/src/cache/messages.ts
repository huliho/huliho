// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AppliedChanges } from "@huliho/core";

// The channel every tab listens on for what the worker changed.
export const CACHE_CHANNEL = "huliho-cache";

// `cleared` names the tab that signed out, so that tab can tell its own
// word from another tab's.
export type CacheMessage =
  ({ kind: "changed"; accountId: string } & AppliedChanges) | { kind: "cleared"; by: string };

function isStringList(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

function changed(fields: ReadonlyMap<string, unknown>): CacheMessage | null {
  const accountId = fields.get("accountId");
  const mailboxes = fields.get("mailboxes");
  const windows = fields.get("windows");
  const threads = fields.get("threads");
  if (typeof accountId !== "string" || typeof mailboxes !== "boolean") {
    return null;
  }
  if (!isStringList(windows) || !isStringList(threads)) {
    return null;
  }
  return { kind: "changed", accountId, mailboxes, windows, threads };
}

// The message as posted, or null for anything else on the channel.
export function readCacheMessage(value: unknown): CacheMessage | null {
  if (typeof value !== "object" || value === null) {
    return null;
  }
  const fields = new Map<string, unknown>(Object.entries(value));
  const kind = fields.get("kind");
  if (kind === "cleared") {
    const by = fields.get("by");
    return typeof by === "string" ? { kind, by } : null;
  }
  return kind === "changed" ? changed(fields) : null;
}
