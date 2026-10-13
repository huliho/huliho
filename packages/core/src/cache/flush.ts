// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { callsFor, outcome } from "../jmap/calls";
import type { Outcome, PatchObject } from "../jmap/calls";
import { JmapError, SERVER_UNAVAILABLE } from "../jmap/client";
import type { JmapClient } from "../jmap/client";
import { setAnswerSchema } from "../jmap/schemas";
import type { z } from "../schema";
import { goneOf } from "./gone";
import { takeBack } from "./pending";
import type { AppliedChanges } from "./poll";
import type { EmailPatch, MailStore, PendingRow } from "./store";

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

type SetAnswer = z.infer<typeof setAnswerSchema>;

function nothing(): AppliedChanges {
  return { mailboxes: false, windows: [], threads: [] };
}

function merged(first: AppliedChanges, second: AppliedChanges): AppliedChanges {
  return {
    mailboxes: first.mailboxes || second.mailboxes,
    windows: [...new Set([...first.windows, ...second.windows])],
    threads: [...new Set([...first.threads, ...second.threads])],
  };
}

function byEmail(rows: readonly PendingRow[]): Map<string, PendingRow[]> {
  const grouped = new Map<string, PendingRow[]>();
  for (const row of rows) {
    grouped.set(row.id, [...(grouped.get(row.id) ?? []), row]);
  }
  return grouped;
}

// One patch per email, a later row winning a path an earlier one named.
function mergedPatch(rows: readonly PendingRow[]): EmailPatch {
  const patch: EmailPatch = {};
  for (const row of rows) {
    Object.assign(patch, row.patch);
  }
  return patch;
}

// Whether the rows of one email add up to nothing: none went out yet,
// none destroys and every path ends where the first row naming it found
// it, as a change and its undo do.
function cancels(rows: readonly PendingRow[]): boolean {
  if (rows.some((row) => row.sentAt !== null || row.destroy === true)) {
    return false;
  }
  const found = new Map<string, true | null>();
  for (const row of rows) {
    for (const [path, value] of Object.entries(row.inverse)) {
      if (!found.has(path)) {
        found.set(path, value);
      }
    }
  }
  return Object.entries(mergedPatch(rows)).every(([path, value]) => found.get(path) === value);
}

// Drops the rows that add up to nothing, so a quick undo costs no
// request; the store already shows what they leave.
async function dropCanceled(store: MailStore, accountId: string): Promise<void> {
  const rows = await store.pending(accountId);
  const dropped = [...byEmail(rows).values()].filter(cancels).flat();
  if (dropped.length > 0) {
    await store.commit(accountId, { pending: { remove: dropped.map((row) => row.seq) } });
  }
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

// The call for the rows sent: an email with a destroy is destroyed and
// its other rows go with it; every other email takes one update.
function callOf(rows: readonly PendingRow[]): {
  update: Record<string, PatchObject>;
  destroy: string[];
} {
  const destroy = [...new Set(rows.filter((row) => row.destroy === true).map(({ id }) => id))];
  const updated = [...byEmail(rows)].filter(([id]) => !destroy.includes(id));
  return {
    update: Object.fromEntries(updated.map(([id, held]) => [id, mergedPatch(held)])),
    destroy,
  };
}

// What the answer says of each email sent: the word of a refused one,
// the ones whose update waits for a server out of reach and keeps its
// rows (RFC 8620 section 3.6.2) and the ones a destroy found destroyed
// or already gone.
interface Verdict {
  refused: Map<string, string>;
  waiting: Set<string>;
  gone: Set<string>;
}

// The type of the SetError an answer names for one object.
function refusalOf(
  errors: Record<string, { type: string }> | null | undefined,
  id: string,
): string {
  return new Map(Object.entries(errors ?? {})).get(id)?.type ?? UNNAMED;
}

// The word for one email: null once the server took it.
function wordOf(answer: SetAnswer, id: string, destroying: boolean): string | null {
  if (destroying) {
    return (answer.destroyed ?? []).includes(id) ? null : refusalOf(answer.notDestroyed, id);
  }
  return Object.hasOwn(answer.updated ?? {}, id) ? null : refusalOf(answer.notUpdated, id);
}

// A method error refuses every email of the call.
function verdictOf(read: Outcome<SetAnswer>, rows: readonly PendingRow[]): Verdict {
  const destroying = new Set(callOf(rows).destroy);
  const verdict: Verdict = { refused: new Map(), waiting: new Set(), gone: new Set() };
  for (const id of new Set(rows.map((row) => row.id))) {
    const word = read.status === "error" ? read.type : wordOf(read.value, id, destroying.has(id));
    if (destroying.has(id) && (word === null || word === NOT_FOUND)) {
      verdict.gone.add(id);
    } else if (word === SERVER_UNAVAILABLE) {
      verdict.waiting.add(id);
    } else if (word !== null) {
      verdict.refused.set(id, word);
    }
  }
  return verdict;
}

// The log after an answer: an acknowledged row leaves, a refused one
// leaves with its change taken back from the rows, a destroyed email
// leaves the store and a waiting one stays for the next flush.
async function settle(
  store: MailStore,
  accountId: string,
  sent: readonly PendingRow[],
  verdict: Verdict,
): Promise<Omit<Flushed, "more">> {
  const { refused, waiting, gone } = verdict;
  const taken = new Set(sent.filter((row) => refused.has(row.id)).map((row) => row.seq));
  const changes = await takeBack(store, accountId, taken);
  const removed = gone.size > 0 ? await goneOf(store, accountId, [...gone]) : null;
  const acknowledged = sent
    .filter((row) => !waiting.has(row.id) && !refused.has(row.id) && !gone.has(row.id))
    .map((row) => row.seq);
  const leaving = [...(removed?.batch.pending?.remove ?? []), ...acknowledged];
  await store.commit(accountId, { ...removed?.batch, pending: { remove: leaving } });
  return {
    changes: merged(changes, removed?.changes ?? nothing()),
    failed: [...refused].filter(([, type]) => type !== NOT_FOUND).map(([id]) => id),
  };
}

// One round: as much of the log as one Email/set takes, sent without a
// state (the last write wins) and settled by the answer. Rows that add
// up to nothing are dropped first and held destroys are passed over. A
// request that brings no answer, or a call answered while the server is
// out of reach, throws and leaves the log for the next flush.
export async function flushPending(client: JmapClient, store: MailStore): Promise<Flushed> {
  const { accountId } = client;
  await dropCanceled(store, accountId);
  const ready = (await store.pending(accountId)).filter((row) => row.held !== true);
  if (ready.length === 0) {
    return { changes: nothing(), failed: [], more: false };
  }
  const session = await client.session();
  const now = Date.now();
  const sent = firstEmails(ready, Math.max(session.maxObjectsInSet, 1));
  for (const row of sent) {
    row.sentAt = now;
  }
  await store.commit(accountId, { pending: { put: sent } });
  const { update, destroy } = callOf(sent);
  const responses = await client.request([
    callsFor(session.accountId).set(
      "Email",
      {
        ...(Object.keys(update).length > 0 ? { update } : {}),
        ...(destroy.length > 0 ? { destroy } : {}),
        ifInState: null,
      },
      "s",
    ),
  ]);
  const read = outcome(responses, "s", setAnswerSchema);
  if (read.status === "error" && read.type === SERVER_UNAVAILABLE) {
    throw new JmapError("unavailable");
  }
  const verdict = verdictOf(read, sent);
  const settled = await settle(store, accountId, sent, verdict);
  return { ...settled, more: verdict.waiting.size === 0 && sent.length < ready.length };
}
