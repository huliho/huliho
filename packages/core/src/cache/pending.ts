// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { memberOf } from "./members";
import { alters, emptied, patched, replayed, unpatched } from "./patch";
import type { AppliedChanges } from "./poll";
import { restate } from "./restate";
import type { EmailPatch, MailStore, MemberState, PendingRow } from "./store";

// A change to an email's keywords and mailboxes, as the user made it. A
// move names its removals and additions in one patch, so the email is
// never in no mailbox (RFC 8621 section 4.1.1). The inverses the log
// answers restore: a thread they bring back into a list takes its place
// there again.
export interface PatchMutation {
  type: "Email";
  // The email id.
  id: string;
  patch: EmailPatch;
  restores?: true;
}

// Delete forever. The email reads as gone at once; the destroy waits,
// held, until the window releases it or takes it back.
export interface DestroyMutation {
  type: "Email";
  id: string;
  destroy: true;
}

export type Mutation = PatchMutation | DestroyMutation;

// What logging some changes did: what the tabs read again, the change
// that takes each patched email back and the held destroys by sequence
// number.
export interface Logged {
  changes: AppliedChanges;
  inverses: PatchMutation[];
  held: number[];
}

function nothing(): Logged {
  return { changes: { mailboxes: false, windows: [], threads: [] }, inverses: [], held: [] };
}

function emptyState(): MemberState {
  return { keywords: {}, mailboxIds: {} };
}

// The row one change logs over a state, without its place in the log;
// null for a change that moves nothing.
type Step = Omit<PendingRow, "seq" | "sentAt">;

function stepOf(state: MemberState, mutation: Mutation, lease: string | undefined): Step | null {
  const destroy = "destroy" in mutation;
  const patch = destroy ? emptied(state) : mutation.patch;
  const { inverse } = patched(state, patch);
  if (!alters(patch, inverse)) {
    return null;
  }
  const row: Step = { type: "Email", id: mutation.id, patch, inverse };
  if (!destroy) {
    return row;
  }
  return { ...row, destroy: true, held: true, ...(lease === undefined ? {} : { lease }) };
}

// The change that takes an email's patches back: every path they named,
// set to what the email held before them.
function inverseOf(id: string, before: MemberState, steps: readonly Step[]): PatchMutation {
  const merged: EmailPatch = {};
  for (const step of steps) {
    Object.assign(merged, step.patch);
  }
  return { type: "Email", id, patch: patched(before, merged).inverse, restores: true };
}

// Changes to several emails, taken by the rows at once and logged until
// the server acknowledges them: the headers, the members of their
// threads, the counts of their mailboxes and the lists they leave. A
// change that moves nothing, names an email the store lacks or one that
// waits to be destroyed is not logged. A held destroy carries the
// `lease` of the tab that asked it.
export async function mutateMany(
  store: MailStore,
  accountId: string,
  mutations: readonly Mutation[],
  lease?: string,
): Promise<Logged> {
  const log = await store.pending(accountId);
  const doomed = new Set(log.filter((row) => row.destroy === true).map((row) => row.id));
  const headers = await store.emails(accountId, [...new Set(mutations.map(({ id }) => id))]);
  const before = new Map([...headers].map(([id, header]) => [id, memberOf(header)]));
  const states = new Map(before);
  const steps: Step[] = [];
  for (const mutation of mutations) {
    const state = states.get(mutation.id);
    const step =
      state === undefined || doomed.has(mutation.id) ? null : stepOf(state, mutation, lease);
    if (state !== undefined && step !== null) {
      states.set(mutation.id, patched(state, step.patch).state);
      steps.push(step);
      if (step.destroy === true) {
        doomed.add(mutation.id);
      }
    }
  }
  if (steps.length === 0) {
    return nothing();
  }
  const changed = new Map(steps.map(({ id }) => [id, states.get(id) ?? emptyState()]));
  const restores = new Set(
    mutations.flatMap((mutation) => ("restores" in mutation ? [mutation.id] : [])),
  );
  const { batch, changes } = await restate(store, accountId, changed, restores);
  const first = (log.at(-1)?.seq ?? 0) + 1;
  const rows = steps.map((step, index): PendingRow => ({
    ...step,
    seq: first + index,
    sentAt: null,
  }));
  await store.commit(accountId, { ...batch, pending: { put: rows } });
  const destroyed = new Set(steps.filter((step) => step.destroy === true).map(({ id }) => id));
  const kept = [...changed.keys()].filter((id) => !destroyed.has(id));
  return {
    changes,
    inverses: kept.map((id) =>
      inverseOf(
        id,
        before.get(id) ?? emptyState(),
        steps.filter((step) => step.id === id),
      ),
    ),
    held: rows.filter((row) => row.held === true).map((row) => row.seq),
  };
}

// Lets the held destroys `picked` names go out with the next flush.
// True when one was released.
async function releaseWhere(
  store: MailStore,
  accountId: string,
  picked: (row: PendingRow) => boolean,
): Promise<boolean> {
  const released = (await store.pending(accountId)).filter(
    (row) => row.held === true && picked(row),
  );
  for (const row of released) {
    row.held = false;
  }
  if (released.length > 0) {
    await store.commit(accountId, { pending: { put: released } });
  }
  return released.length > 0;
}

// The window's toast closed or ran out: the destroys go out.
export function releaseHeld(
  store: MailStore,
  accountId: string,
  seqs: readonly number[],
): Promise<boolean> {
  return releaseWhere(store, accountId, (row) => seqs.includes(row.seq));
}

// Every held destroy whose tab holds no live lease goes out, the ones
// without a lease too; with no live lease that is every held one, as at
// the worker's start.
export function releaseLapsed(
  store: MailStore,
  accountId: string,
  live: ReadonlySet<string>,
): Promise<boolean> {
  return releaseWhere(store, accountId, (row) => row.lease === undefined || !live.has(row.lease));
}

// Takes the rows `leaving` names out of the log. Each email they change
// returns to what the server holds with its other rows laid over it
// again, and takes its place back in the lists it left.
export async function takeBack(
  store: MailStore,
  accountId: string,
  leaving: ReadonlySet<number>,
): Promise<AppliedChanges> {
  const log = await store.pending(accountId);
  const ids = [...new Set(log.filter((row) => leaving.has(row.seq)).map((row) => row.id))];
  if (ids.length === 0) {
    return nothing().changes;
  }
  const states = new Map<string, MemberState>();
  const kept: PendingRow[] = [];
  for (const [id, header] of await store.emails(accountId, ids)) {
    const rows = log.filter((row) => row.id === id);
    const replay = replayed(
      unpatched(memberOf(header), rows),
      rows.filter((row) => !leaving.has(row.seq)),
    );
    states.set(id, replay.state);
    kept.push(...replay.rows);
  }
  const { batch, changes } = await restate(store, accountId, states, new Set(ids));
  await store.commit(accountId, { ...batch, pending: { put: kept, remove: [...leaving] } });
  return changes;
}

// Undo of a delete forever: the held destroys leave the log and their
// emails show again where they stood. A destroy the window released is
// past taking back.
export async function dropHeld(
  store: MailStore,
  accountId: string,
  seqs: readonly number[],
): Promise<AppliedChanges> {
  const log = await store.pending(accountId);
  const dropping = log.filter((row) => row.held === true && seqs.includes(row.seq));
  return takeBack(store, accountId, new Set(dropping.map((row) => row.seq)));
}
