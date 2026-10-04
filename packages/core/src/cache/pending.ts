// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { callsFor, outcome } from "../jmap/calls";
import type { Outcome, PatchObject } from "../jmap/calls";
import { JmapError, SERVER_UNAVAILABLE } from "../jmap/client";
import type { JmapClient } from "../jmap/client";
import { setAnswerSchema } from "../jmap/schemas";
import type { EmailHeader } from "../jmap/schemas";
import type { z } from "../schema";
import { memberOf } from "./members";
import { alters, patched, unpatched } from "./patch";
import type { AppliedChanges } from "./poll";
import type { Batch, EmailPatch, MailStore, MemberState, PendingRow, ThreadRow } from "./store";
import { movesOf, recounted } from "./unread";

// One change to an email, as the user made it.
export interface Mutation {
  type: "Email";
  // The email id.
  id: string;
  patch: EmailPatch;
}

// What one round of a flush settled: the rows a refusal put back, which
// the tabs read again, plus the emails whose change the server refused.
export interface Flushed {
  changes: AppliedChanges;
  failed: string[];
  // Whether a next round would send rows this one did not. False while
  // an update waits for a server out of reach, since the next round
  // starts with that update again.
  more: boolean;
}

// The SetError of an email that is gone (RFC 8620 section 5.3); its
// change has nothing left to stand on and nobody to tell.
const NOT_FOUND = "notFound";

// The word for an update the answer names nowhere.
const UNNAMED = "serverFail";

function nothing(): AppliedChanges {
  return { mailboxes: false, windows: [], threads: [] };
}

// The thread row with the state of the members that changed.
function withMembers(thread: ThreadRow, changed: ReadonlyMap<string, MemberState>): ThreadRow {
  return {
    ...thread,
    members: Object.fromEntries(
      Object.entries(thread.members).map(([id, member]) => [id, changed.get(id) ?? member]),
    ),
  };
}

// The rows as they stand once some emails take other keywords: their
// headers, the members of their threads and the unread counts of their
// mailboxes.
async function rekeyed(
  store: MailStore,
  accountId: string,
  keywords: ReadonlyMap<string, Record<string, true>>,
): Promise<{ batch: Batch; changes: AppliedChanges }> {
  const held = [...(await store.emails(accountId, [...keywords.keys()])).values()];
  const headers = held.map((header): EmailHeader => ({
    ...header,
    keywords: keywords.get(header.id) ?? header.keywords,
  }));
  const after = new Map(headers.map((header) => [header.id, memberOf(header)]));
  const threadIds = [...new Set(held.map((header) => header.threadId))];
  const threads = [...(await store.threads(accountId, threadIds)).values()];
  const moved = movesOf(
    new Map(threads.map((thread) => [thread.id, thread.members])),
    held.map((header) => ({
      id: header.id,
      threadId: header.threadId,
      before: memberOf(header),
      after: after.get(header.id) ?? memberOf(header),
    })),
  );
  const mailboxes = recounted(await store.mailboxes(accountId), moved);
  const members = threads.map((thread) => withMembers(thread, after));
  return {
    batch: { emails: { put: headers }, threads: { put: members }, mailboxes: { put: mailboxes } },
    changes: {
      mailboxes: mailboxes.length > 0,
      windows: [...new Set(held.flatMap((header) => Object.keys(header.mailboxIds)))],
      threads: threadIds,
    },
  };
}

// One change, taken by the rows at once and logged until the server
// acknowledges it: the header, the member of its thread and the unread
// counts of its mailboxes. A change that moves nothing is not logged.
export async function applyPatch(
  store: MailStore,
  accountId: string,
  mutation: Mutation,
): Promise<AppliedChanges> {
  const header = (await store.emails(accountId, [mutation.id])).get(mutation.id);
  if (header === undefined) {
    return nothing();
  }
  const { keywords, inverse } = patched(header.keywords, mutation.patch);
  if (!alters(mutation.patch, inverse)) {
    return nothing();
  }
  const { batch, changes } = await rekeyed(store, accountId, new Map([[header.id, keywords]]));
  const last = (await store.pending(accountId)).at(-1);
  const row: PendingRow = {
    seq: (last?.seq ?? 0) + 1,
    type: "Email",
    id: header.id,
    patch: mutation.patch,
    inverse,
    sentAt: null,
  };
  await store.commit(accountId, { ...batch, pending: { put: [row] } });
  return changes;
}

// The rows of the first `emails` emails the log names, every row of
// each: one /set takes no more objects than the server allows.
function firstEmails(rows: readonly PendingRow[], emails: number): PendingRow[] {
  const ids = new Set<string>();
  for (const row of rows) {
    if (ids.size < emails) {
      ids.add(row.id);
    }
  }
  return rows.filter((row) => ids.has(row.id));
}

// One patch per email, a later row winning a path an earlier one named.
function updatesOf(rows: readonly PendingRow[]): Record<string, PatchObject> {
  const updates = new Map<string, EmailPatch>();
  for (const row of rows) {
    updates.set(row.id, { ...updates.get(row.id), ...row.patch });
  }
  return Object.fromEntries(updates);
}

// What the answer says of the emails it does not name as updated: the
// word the server gave each refused one, and the ones whose update
// waits for a server out of reach and keeps its rows (RFC 8620 section
// 3.6.2).
interface Verdict {
  refused: Map<string, string>;
  waiting: Set<string>;
}

// A method error refuses every email of the call.
function verdictOf(
  read: Outcome<z.infer<typeof setAnswerSchema>>,
  rows: readonly PendingRow[],
): Verdict {
  const ids = [...new Set(rows.map((row) => row.id))];
  if (read.status === "error") {
    return { refused: new Map(ids.map((id) => [id, read.type])), waiting: new Set() };
  }
  const updated = new Set(Object.keys(read.value.updated ?? {}));
  const errors = new Map(Object.entries(read.value.notUpdated ?? {}));
  const words = ids
    .filter((id) => !updated.has(id))
    .map((id): [string, string] => [id, errors.get(id)?.type ?? UNNAMED]);
  return {
    refused: new Map(words.filter(([, type]) => type !== SERVER_UNAVAILABLE)),
    waiting: new Set(words.filter(([, type]) => type === SERVER_UNAVAILABLE).map(([id]) => id)),
  };
}

// The log after an answer: an acknowledged row leaves, a refused one
// leaves with its patch taken back from the rows and a waiting one
// stays for the next flush.
async function settle(
  store: MailStore,
  accountId: string,
  sent: readonly PendingRow[],
  { refused, waiting }: Verdict,
): Promise<Omit<Flushed, "more">> {
  const held = await store.emails(accountId, [...refused.keys()]);
  const keywords = new Map(
    [...held.values()].map((header) => [
      header.id,
      unpatched(
        header.keywords,
        sent.filter((row) => row.id === header.id),
      ),
    ]),
  );
  const { batch, changes } = await rekeyed(store, accountId, keywords);
  const leaving = sent.filter((row) => !waiting.has(row.id)).map((row) => row.seq);
  await store.commit(accountId, { ...batch, pending: { remove: leaving } });
  return {
    changes,
    failed: [...refused].filter(([, type]) => type !== NOT_FOUND).map(([id]) => id),
  };
}

// One round: as much of the log as one Email/set takes, sent without a
// state (the last write wins) and settled by the answer. A request that
// brings no answer, or a call answered while the server is out of
// reach, throws and leaves the log for the next flush.
export async function flushPending(client: JmapClient, store: MailStore): Promise<Flushed> {
  const { accountId } = client;
  const rows = await store.pending(accountId);
  if (rows.length === 0) {
    return { changes: nothing(), failed: [], more: false };
  }
  const session = await client.session();
  const now = Date.now();
  const sent = firstEmails(rows, Math.max(session.maxObjectsInSet, 1));
  for (const row of sent) {
    row.sentAt = now;
  }
  await store.commit(accountId, { pending: { put: sent } });
  const calls = callsFor(session.accountId);
  const responses = await client.request([
    calls.set("Email", { update: updatesOf(sent), ifInState: null }, "s"),
  ]);
  const read = outcome(responses, "s", setAnswerSchema);
  if (read.status === "error" && read.type === SERVER_UNAVAILABLE) {
    throw new JmapError("unavailable");
  }
  const verdict = verdictOf(read, sent);
  const settled = await settle(store, accountId, sent, verdict);
  return { ...settled, more: verdict.waiting.size === 0 && sent.length < rows.length };
}
