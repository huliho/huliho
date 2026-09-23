// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ChangesAnswer, EmailHeader, EmailState, Thread } from "../jmap/schemas";
import { memberOf, threadRow } from "./members";
import type { Batch, QueryRow, ThreadRow } from "./store";

// What one round of changes did to the store.
export interface Folded {
  mailboxes: boolean;
  // The threads updated or destroyed.
  threads: string[];
  // The lists whose rows changed or left.
  lists: string[];
  // Whether the order or the membership of a list may have moved.
  listMoved: boolean;
  // Whether the emails, threads and lists were dropped for a refetch.
  reset: boolean;
}

export function nothingFolded(): Folded {
  return { mailboxes: false, threads: [], lists: [], listMoved: false, reset: false };
}

// The batch a round builds and the account of what it did.
export interface Landing {
  batch: Batch;
  folded: Folded;
}

// The rows a round reads before it lands, as copies to edit.
export interface Held {
  headers: Map<string, EmailHeader>;
  heldThreads: Map<string, ThreadRow>;
  queries: QueryRow[];
}

// An email that left a mailbox, by the lists it stood in.
interface Left {
  id: string;
  from: string[];
}

// The states patch the header rows and the thread members held; a
// destroyed email and one that left a mailbox leave every list they
// stood in.
export function landEmails(
  changes: ChangesAnswer,
  states: readonly EmailState[],
  held: Held,
  { batch, folded }: Landing,
): void {
  const { patched, left } = patchHeaders(states, held, folded);
  const gone = new Set(changes.destroyed);
  for (const row of held.queries) {
    const leaving = left.filter((it) => it.from.includes(row.id)).map((it) => it.id);
    if (strip(row, new Set([...gone, ...leaving]))) {
      folded.lists.push(row.id);
      batch.queries = { put: [...(batch.queries?.put ?? []), row] };
    }
  }
  batch.emails = { put: patched, remove: changes.destroyed };
  batch.states = { ...batch.states, Email: changes.newState };
  folded.listMoved ||= changes.created.length > 0 || changes.destroyed.length > 0;
}

// An email the store does not hold may still be new to a list it names.
function patchHeaders(
  states: readonly EmailState[],
  held: Held,
  folded: Folded,
): { patched: EmailHeader[]; left: Left[] } {
  const lists = new Set(held.queries.map((row) => row.id));
  const patched: EmailHeader[] = [];
  const left: Left[] = [];
  for (const state of states) {
    const thread = held.heldThreads.get(state.threadId);
    if (thread !== undefined) {
      thread.members = { ...thread.members, [state.id]: memberOf(state) };
    }
    const header = held.headers.get(state.id);
    if (header === undefined) {
      folded.listMoved ||= Object.keys(state.mailboxIds).some((id) => lists.has(id));
      continue;
    }
    patched.push({ ...header, ...state });
    const from = Object.keys(header.mailboxIds).filter((id) => !(id in state.mailboxIds));
    const into = Object.keys(state.mailboxIds).some((id) => !(id in header.mailboxIds));
    if (from.length > 0 || into) {
      folded.listMoved = true;
      left.push({ id: state.id, from });
    }
  }
  return { patched, left };
}

// The threads a round updates, with the states of their members.
export interface ThreadUpdate {
  changes: ChangesAnswer;
  threads: readonly Thread[];
  members: ReadonlyMap<string, EmailState>;
}

// An updated thread is rebuilt from the server's order of its emails.
export function landThreads(
  update: ThreadUpdate,
  heldThreads: Map<string, ThreadRow>,
  { batch, folded }: Landing,
): void {
  const { changes } = update;
  for (const row of update.threads) {
    heldThreads.set(row.id, threadRow(row, update.members, heldThreads.get(row.id)));
  }
  for (const id of changes.destroyed) {
    heldThreads.delete(id);
  }
  batch.threads = { ...batch.threads, remove: changes.destroyed };
  batch.states = { ...batch.states, Thread: changes.newState };
  folded.threads = [...changes.updated, ...changes.destroyed];
  folded.listMoved ||= folded.threads.length > 0 || changes.created.length > 0;
}

// Takes `ids` out of every page of the list; true when a row left.
function strip(row: QueryRow, ids: ReadonlySet<string>): boolean {
  let left = false;
  for (const page of row.pages) {
    const kept = page.ids.filter((id) => !ids.has(id));
    if (kept.length !== page.ids.length) {
      row.total =
        row.total === null ? null : Math.max(0, row.total - page.ids.length + kept.length);
      page.ids = kept;
      left = true;
    }
  }
  row.pending = row.pending.filter((id) => !ids.has(id));
  if (row.fresh !== null) {
    row.fresh.ids = row.fresh.ids.filter((id) => !ids.has(id));
  }
  return left;
}

export function mergeFolded(first: Folded, second: Folded): Folded {
  return {
    mailboxes: first.mailboxes || second.mailboxes,
    threads: [...new Set([...first.threads, ...second.threads])],
    lists: [...new Set([...first.lists, ...second.lists])],
    listMoved: first.listMoved || second.listMoved,
    reset: first.reset || second.reset,
  };
}
