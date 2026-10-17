// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// Email/set over the corpus, as RFC 8621 section 4.6 has it: keyword
// patches, moves between mailboxes and destroys, each logged so the
// next poll sees it, with every mailbox it touches counted again.

import type { MailServer } from "./mail-answers";
import { UPSTREAM } from "./mail-corpus";
import type { CorpusEmail } from "./mail-corpus";

type Args = Record<string, unknown>;

const KEYWORDS = "keywords/";
const MAILBOXES = "mailboxIds/";

// The keywords a corpus message carries and how a patch sets them; the
// two junk keywords a spam action writes are taken and kept nowhere.
const KNOWN_KEYWORDS = new Map<string, (message: CorpusEmail, on: boolean) => CorpusEmail>([
  ["$seen", (message, on) => ({ ...message, seen: on })],
  ["$flagged", (message, on) => ({ ...message, flagged: on })],
  ["$junk", (message) => message],
  ["$notjunk", (message) => message],
]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function stateOf(server: MailServer): string {
  return String(server.changes.length);
}

function logged(
  server: MailServer,
  type: "Email" | "Thread" | "Mailbox",
  id: string,
  kind: "updated" | "destroyed",
): void {
  server.changes.push({ sequence: server.changes.length + 1, type, id, kind });
}

// The mailbox a message stands in after a patch, null for a move this
// server cannot make: a corpus message stands in one mailbox, so a move
// names its removal and its addition together.
function movedTo(
  server: MailServer,
  message: CorpusEmail,
  paths: [string, unknown][],
): string | null {
  const added = paths
    .filter(([, value]) => value === true)
    .map(([path]) => path.slice(MAILBOXES.length));
  const removed = paths
    .filter(([, value]) => value === null)
    .map(([path]) => path.slice(MAILBOXES.length));
  if (added.length === 0 && removed.length === 0) {
    return message.mailboxId;
  }
  const to = added[0];
  const known = server.mailboxes.some((row) => row["id"] === to);
  return added.length === 1 &&
    removed.length === 1 &&
    removed[0] === message.mailboxId &&
    known &&
    to !== undefined
    ? to
    : null;
}

// The message after a patch, each path true or null (RFC 8620 section
// 5.3); null for a patch that names anything else or leaves the message
// in no mailbox.
function patched(server: MailServer, message: CorpusEmail, patch: Args): CorpusEmail | null {
  const entries = Object.entries(patch);
  const moved = movedTo(
    server,
    message,
    entries.filter(([path]) => path.startsWith(MAILBOXES)),
  );
  if (moved === null) {
    return null;
  }
  let next = { ...message, mailboxId: moved };
  for (const [path, value] of entries.filter(([key]) => !key.startsWith(MAILBOXES))) {
    const set = KNOWN_KEYWORDS.get(path.slice(KEYWORDS.length));
    if (!path.startsWith(KEYWORDS) || set === undefined || (value !== true && value !== null)) {
      return null;
    }
    next = set(next, value === true);
  }
  return next;
}

// The first message of each thread among a list's messages, newest first.
function exemplarsOf(server: MailServer, ids: readonly string[]): string[] {
  const seen = new Set<string>();
  return ids.filter((id) => {
    const threadId = server.corpus.emails.get(id)?.threadId;
    if (threadId === undefined || seen.has(threadId)) {
      return false;
    }
    seen.add(threadId);
    return true;
  });
}

// A mailbox's list with `id` taken out, or put in at its place by the
// time it arrived.
function relisted(server: MailServer, mailboxId: string, id: string, entering: boolean): void {
  const ids = (server.corpus.lists.get(mailboxId)?.ids ?? []).filter((held) => held !== id);
  if (entering) {
    const at = server.corpus.emails.get(id)?.receivedAt ?? "";
    const place = ids.findIndex((held) => (server.corpus.emails.get(held)?.receivedAt ?? "") < at);
    ids.splice(place === -1 ? ids.length : place, 0, id);
  }
  server.corpus.lists.set(mailboxId, { ids, exemplars: exemplarsOf(server, ids) });
}

// Brings a mailbox row's four counts in line with its messages; a row
// whose counts move is logged as updated.
function recount(server: MailServer, mailboxId: string): void {
  const row = server.mailboxes.find((one) => one["id"] === mailboxId);
  const list = server.corpus.lists.get(mailboxId);
  if (row === undefined || list === undefined) {
    return;
  }
  const unread = list.ids.flatMap((id) => {
    const message = server.corpus.emails.get(id);
    return message !== undefined && !message.seen ? [message] : [];
  });
  const counts = {
    totalEmails: list.ids.length,
    unreadEmails: unread.length,
    totalThreads: list.exemplars.length,
    unreadThreads: new Set(unread.map((one) => one.threadId)).size,
  };
  const before = JSON.stringify(row);
  Object.assign(row, counts);
  if (JSON.stringify(row) !== before) {
    logged(server, "Mailbox", mailboxId, "updated");
  }
}

// One update: the SetError type when it is refused, null once written.
function updated(server: MailServer, id: string, patch: unknown): string | null {
  const message = server.corpus.emails.get(id);
  const refusal = server.refused.get(id);
  if (refusal !== undefined || message === undefined || server.gone.has(id)) {
    return refusal ?? "notFound";
  }
  const next = isRecord(patch) ? patched(server, message, patch) : null;
  if (next === null) {
    return "invalidProperties";
  }
  server.corpus.emails.set(id, next);
  logged(server, "Email", id, "updated");
  if (next.mailboxId !== message.mailboxId) {
    relisted(server, message.mailboxId, id, false);
    relisted(server, next.mailboxId, id, true);
    recount(server, next.mailboxId);
  }
  recount(server, message.mailboxId);
  return null;
}

// One destroy: the SetError type when it is refused, null once done. The
// message leaves its mailbox and its thread, the thread with its last.
function destroyed(server: MailServer, id: string): string | null {
  const message = server.corpus.emails.get(id);
  const refusal = server.refused.get(id);
  if (refusal !== undefined || message === undefined || server.gone.has(id)) {
    return refusal ?? "notFound";
  }
  relisted(server, message.mailboxId, id, false);
  server.corpus.emails.delete(id);
  const left = (server.corpus.threads.get(message.threadId) ?? []).filter((held) => held !== id);
  if (left.length === 0) {
    server.corpus.threads.delete(message.threadId);
  } else {
    server.corpus.threads.set(message.threadId, left);
  }
  logged(server, "Email", id, "destroyed");
  logged(server, "Thread", message.threadId, left.length === 0 ? "destroyed" : "updated");
  recount(server, message.mailboxId);
  return null;
}

function named<Value>(entries: [string, Value][]): Record<string, Value> | null {
  return entries.length === 0 ? null : Object.fromEntries(entries);
}

function refusals(outcomes: [string, string | null][]): Record<string, { type: string }> | null {
  return named(
    outcomes.flatMap(([id, refusal]): [string, { type: string }][] =>
      refusal === null ? [] : [[id, { type: refusal }]],
    ),
  );
}

export function emailSet(server: MailServer, args: Args): Args {
  server.sets.push(args);
  const oldState = stateOf(server);
  const updates = isRecord(args["update"]) ? Object.entries(args["update"]) : [];
  const outcomes = updates.map(([id, patch]): [string, string | null] => [
    id,
    updated(server, id, patch),
  ]);
  const destroys = Array.isArray(args["destroy"])
    ? args["destroy"].filter((id) => typeof id === "string")
    : [];
  const gone = destroys.map((id): [string, string | null] => [id, destroyed(server, id)]);
  return {
    accountId: UPSTREAM,
    oldState,
    newState: stateOf(server),
    created: null,
    updated: named(outcomes.flatMap(([id, refusal]) => (refusal === null ? [[id, null]] : []))),
    destroyed: gone.flatMap(([id, refusal]) => (refusal === null ? [id] : [])),
    notCreated: null,
    notUpdated: refusals(outcomes),
    notDestroyed: refusals(gone),
  };
}
