// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AppliedChanges, ChangeVerb, JobProgress, StopCause } from "@huliho/core";

// The channel every tab listens on for what the worker changed.
export const CACHE_CHANNEL = "huliho-cache";

// What the worker tells every tab.
export type CacheMessage =
  | ({ kind: "changed"; accountId: string } & AppliedChanges)
  // The tab that signed out, so it can tell its own word from another's.
  | { kind: "cleared"; by: string }
  // The server stopped the account or runs it again, as a poll found it.
  | { kind: "account"; accountId: string; stoppedCause: StopCause | null }
  // The server refused changes to these emails and the rows gave them
  // back; the verbs name what the changes did.
  | { kind: "refused"; accountId: string; emailIds: string[]; verbs: ChangeVerb[] }
  // Where a folder job of the account stands after a round.
  | { kind: "job"; accountId: string; progress: JobProgress }
  // The worker moved to an empty store, so every mail query reads again.
  | { kind: "reset" }
  // A worker keeps mail off the disk, as the privacy setting asks.
  | { kind: "strict" };

type Fields = ReadonlyMap<string, unknown>;

const STOP_CAUSES: readonly StopCause[] = ["credentials", "connection"];

const VERBS: ReadonlySet<unknown> = new Set<ChangeVerb>([
  "archive",
  "delete",
  "deleteForever",
  "spam",
  "notSpam",
  "read",
  "unread",
  "star",
  "unstar",
  "move",
]);

function isStringList(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

function isStopCause(value: unknown): value is StopCause | null {
  return value === null || STOP_CAUSES.some((cause) => cause === value);
}

function isVerbList(value: unknown): value is ChangeVerb[] {
  return Array.isArray(value) && value.every((item) => VERBS.has(item));
}

function account(fields: Fields): CacheMessage | null {
  const accountId = fields.get("accountId");
  const stoppedCause = fields.get("stoppedCause");
  if (typeof accountId !== "string" || !isStopCause(stoppedCause)) {
    return null;
  }
  return { kind: "account", accountId, stoppedCause };
}

function changed(fields: Fields): CacheMessage | null {
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

function refused(fields: Fields): CacheMessage | null {
  const accountId = fields.get("accountId");
  const emailIds = fields.get("emailIds");
  const verbs = fields.get("verbs");
  if (typeof accountId !== "string" || !isStringList(emailIds) || !isVerbList(verbs)) {
    return null;
  }
  return { kind: "refused", accountId, emailIds, verbs };
}

function cleared(fields: Fields): CacheMessage | null {
  const by = fields.get("by");
  return typeof by === "string" ? { kind: "cleared", by } : null;
}

// The messages a tab reads; a job's progress waits for the view that
// shows it.
const READERS: ReadonlyMap<unknown, (fields: Fields) => CacheMessage | null> = new Map([
  ["cleared", cleared],
  ["account", account],
  ["changed", changed],
  ["refused", refused],
  ["reset", (): CacheMessage => ({ kind: "reset" })],
  ["strict", (): CacheMessage => ({ kind: "strict" })],
]);

// The message as posted, or null for anything else on the channel.
export function readCacheMessage(value: unknown): CacheMessage | null {
  if (typeof value !== "object" || value === null) {
    return null;
  }
  const fields = new Map<string, unknown>(Object.entries(value));
  return READERS.get(fields.get("kind"))?.(fields) ?? null;
}
