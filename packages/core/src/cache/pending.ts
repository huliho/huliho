// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { knownEmails } from "./known";
import type { KnownEmail } from "./known";
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
  // The thread it stands in, through which the log finds an email the
  // store holds no header for.
  threadId?: string;
  patch: EmailPatch;
  restores?: true;
}

// Delete forever. The email reads as gone at once; the destroy waits,
// held, until the window releases it or takes it back.
export interface DestroyMutation {
  type: "Email";
  id: string;
  threadId?: string;
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

function stepOf(
  state: MemberState,
  mutation: Mutation,
  { threadId, lease }: { threadId: string; lease: string | undefined },
): Step | null {
  const destroy = "destroy" in mutation;
  const patch = destroy ? emptied(state) : mutation.patch;
  const { inverse } = patched(state, patch);
  if (!alters(patch, inverse)) {
    return null;
  }
  const row: Step = { type: "Email", id: mutation.id, threadId, patch, inverse };
  if (!destroy) {
    return row;
  }
  return { ...row, destroy: true, held: true, ...(lease === undefined ? {} : { lease }) };
}

// The change that takes an email's patches back: every path they named,
// set to what the email held before them.
function inverseOf(
  { id, threadId }: { id: string; threadId: string },
  before: MemberState,
  steps: readonly Step[],
): PatchMutation {
  const merged: EmailPatch = {};
  for (const step of steps) {
    Object.assign(merged, step.patch);
  }
  return { type: "Email", id, threadId, patch: patched(before, merged).inverse, restores: true };
}

// The rows a call logs, one per change that moves something, each over
// the state the changes before it left; the states they leave; each
// email logged with its thread, in the order the changes first name it.
// A destroy logged dooms its email for the rest of the call.
function stepsOf(
  mutations: readonly Mutation[],
  known: ReadonlyMap<string, KnownEmail>,
  { doomed, lease }: { doomed: Set<string>; lease: string | undefined },
): { steps: Step[]; states: Map<string, MemberState>; logged: Map<string, string> } {
  const states = new Map([...known].map(([id, email]) => [id, email.state]));
  const steps: Step[] = [];
  const logged = new Map<string, string>();
  for (const mutation of mutations) {
    const email = known.get(mutation.id);
    const state = states.get(mutation.id);
    if (email === undefined || state === undefined || doomed.has(mutation.id)) {
      continue;
    }
    const step = stepOf(state, mutation, { threadId: email.threadId, lease });
    if (step !== null) {
      states.set(mutation.id, patched(state, step.patch).state);
      steps.push(step);
      logged.set(mutation.id, email.threadId);
      if (step.destroy === true) {
        doomed.add(mutation.id);
      }
    }
  }
  return { steps, states, logged };
}

// Changes to several emails, taken by the rows at once and logged until
// the server acknowledges them: the headers, the members of their
// threads, the counts of their mailboxes and the lists they leave. The
// log skips a change that moves nothing. It skips a change to an email
// the store knows by neither header nor thread. It skips a change to an
// email that already waits to be destroyed. A held destroy carries the
// `lease` of the tab that asked it.
export async function mutateMany(
  store: MailStore,
  accountId: string,
  mutations: readonly Mutation[],
  lease?: string,
): Promise<Logged> {
  const log = await store.pending(accountId);
  const known = await knownEmails(store, accountId, mutations);
  const before = new Map([...known].map(([id, email]) => [id, email.state]));
  const { steps, states, logged } = stepsOf(mutations, known, {
    doomed: new Set(log.filter((row) => row.destroy === true).map((row) => row.id)),
    lease,
  });
  if (steps.length === 0) {
    return nothing();
  }
  const changed = new Map([...logged.keys()].map((id) => [id, states.get(id) ?? emptyState()]));
  const restores = new Set(
    mutations.flatMap((mutation) => ("restores" in mutation ? [mutation.id] : [])),
  );
  const { batch, changes } = await restate(store, accountId, changed, { known, restores });
  const first = (log.at(-1)?.seq ?? 0) + 1;
  const rows = steps.map((step, index): PendingRow => ({
    ...step,
    seq: first + index,
    sentAt: null,
  }));
  await store.commit(accountId, { ...batch, pending: { put: rows } });
  const destroyed = new Set(steps.filter((step) => step.destroy === true).map(({ id }) => id));
  const kept = [...logged].filter(([id]) => !destroyed.has(id));
  return {
    changes,
    inverses: kept.map(([id, threadId]) =>
      inverseOf(
        { id, threadId },
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
  const taken = log.filter((row) => leaving.has(row.seq));
  if (taken.length === 0) {
    return nothing().changes;
  }
  const known = await knownEmails(store, accountId, taken);
  const states = new Map<string, MemberState>();
  const kept: PendingRow[] = [];
  for (const [id, email] of known) {
    const rows = log.filter((row) => row.id === id);
    const replay = replayed(
      unpatched(email.state, rows),
      rows.filter((row) => !leaving.has(row.seq)),
    );
    states.set(id, replay.state);
    kept.push(...replay.rows);
  }
  const restores = new Set(taken.map(({ id }) => id));
  const { batch, changes } = await restate(store, accountId, states, { known, restores });
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
