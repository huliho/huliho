// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { answer, callsFor, outcome, reference } from "../jmap/calls";
import type { PatchObject } from "../jmap/calls";
import { JmapError, MethodFailure, SERVER_UNAVAILABLE } from "../jmap/client";
import type { JmapClient } from "../jmap/client";
import {
  getAnswerSchema,
  idSchema,
  queryAnswerSchema,
  setAnswerSchema,
  setSchema,
} from "../jmap/schemas";
import type { Invocation } from "../jmap/schemas";
import { z } from "../schema";
import { mailboxPass } from "./changes";
import { SEEN } from "./counts";
import { goneOf } from "./gone";
import { listsAfter } from "./lists";
import { memberOf } from "./members";
import { land } from "./overlay";
import { patched, pathOf, unpatched } from "./patch";
import type { AppliedChanges } from "./poll";
import type { EmailPatch, MailStore, MemberState, ThreadRow } from "./store";

// A folder job: every email of a mailbox marked read, or the mailbox
// emptied.
export interface Job {
  kind: "markRead" | "empty";
  mailboxId: string;
}

export type JobState = "running" | "done" | "canceled" | "failed";

// Where a job stands: the emails it finished, how many it found when it
// started (null before the first round), the first email the server
// took in the round before (null before any round) and its state.
export interface JobProgress {
  job: Job;
  done: number;
  total: number | null;
  // A round that reaches this email again fails, since the server did
  // not apply the write of the round before.
  last: string | null;
  state: JobState;
}

// What one round did: where the job stands after it and what the tabs
// read again.
export interface JobRound {
  progress: JobProgress;
  changes: AppliedChanges;
}

const mailboxIdsAnswerSchema = getAnswerSchema(z.object({ id: idSchema, mailboxIds: setSchema }));

function nothing(): AppliedChanges {
  return { mailboxes: false, windows: [], threads: [] };
}

// The patch a job writes on each email it reaches.
function patchOf(job: Job): EmailPatch {
  return job.kind === "markRead"
    ? { [pathOf("keywords", SEEN)]: true }
    : { [pathOf("mailboxIds", job.mailboxId)]: null };
}

// The emails a round reaches: the next ones the job has left to do,
// newest first, each with the mailboxes it stands in when it empties.
// Reaching the email the round before took means the server did not
// apply that write, which fails the round.
interface Reached {
  ids: string[];
  total: number | null;
  mailboxIds: Map<string, Record<string, true>>;
}

async function reach(client: JmapClient, progress: JobProgress, limit: number): Promise<Reached> {
  const { job } = progress;
  const calls = callsFor((await client.session()).accountId);
  const window = {
    filter:
      job.kind === "markRead"
        ? { inMailbox: job.mailboxId, notKeyword: SEEN }
        : { inMailbox: job.mailboxId },
    start: { position: 0 },
    limit,
    calculateTotal: progress.total === null,
    collapseThreads: false,
  };
  const responses = await client.request([
    calls.query(window, "q"),
    ...(job.kind === "empty"
      ? [calls.get("Email", reference("q", "Email/query", "/ids"), "g", ["mailboxIds"])]
      : []),
  ]);
  const query = answer(responses, "q", queryAnswerSchema);
  if (progress.last !== null && query.ids.includes(progress.last)) {
    throw new MethodFailure("serverFail", "q");
  }
  const listed = job.kind === "empty" ? answer(responses, "g", mailboxIdsAnswerSchema).list : [];
  return {
    ids: query.ids,
    total: query.total ?? null,
    mailboxIds: new Map(listed.map((row) => [row.id, row.mailboxIds])),
  };
}

// The write of one round. Marking read sets $seen; emptying destroys an
// email whose only mailbox is the one emptied and takes that mailbox
// from the rest, as RFC 8621 section 4.6 asks of a client emptying
// Trash.
function writeOf(
  job: Job,
  reached: Reached,
): { update: Record<string, PatchObject>; destroy: string[] } {
  const patch = patchOf(job);
  if (job.kind === "markRead") {
    return { update: Object.fromEntries(reached.ids.map((id) => [id, patch])), destroy: [] };
  }
  const only = (id: string): boolean => {
    const boxes = Object.keys(reached.mailboxIds.get(id) ?? {});
    return boxes.length === 1 && boxes[0] === job.mailboxId;
  };
  const kept = reached.ids.filter((id) => reached.mailboxIds.has(id) && !only(id));
  return {
    update: Object.fromEntries(kept.map((id) => [id, patch])),
    destroy: reached.ids.filter(only),
  };
}

// What the server took of the write; a call refused whole fails the
// round, and so does a server out of reach.
function acknowledged(responses: readonly Invocation[]): {
  updated: string[];
  destroyed: string[];
} {
  const read = outcome(responses, "s", setAnswerSchema);
  if (read.status === "error") {
    if (read.type === SERVER_UNAVAILABLE) {
      throw new JmapError("unavailable");
    }
    throw new MethodFailure(read.type, "s");
  }
  return { updated: Object.keys(read.value.updated ?? {}), destroyed: read.value.destroyed ?? [] };
}

function withServed(thread: ThreadRow, served: ReadonlyMap<string, MemberState>): ThreadRow {
  return {
    ...thread,
    members: Object.fromEntries(
      Object.entries(thread.members).map(([id, member]) => [id, served.get(id) ?? member]),
    ),
  };
}

// The emails the server updated land as it now holds them, with every
// change still logged laid over them again; a list they left drops
// them. Answers their threads.
async function landUpdated(
  store: MailStore,
  accountId: string,
  ids: readonly string[],
  patch: EmailPatch,
): Promise<string[]> {
  const log = await store.pending(accountId);
  const held = [...(await store.emails(accountId, ids)).values()];
  const served = new Map(
    held.map((header): [string, MemberState] => {
      const rows = log.filter((row) => row.id === header.id);
      return [header.id, patched(unpatched(memberOf(header), rows), patch).state];
    }),
  );
  const threadIds = [...new Set(held.map((header) => header.threadId))];
  const threads = [...(await store.threads(accountId, threadIds)).values()];
  const changes = held.map((header) => ({
    id: header.id,
    threadId: header.threadId,
    receivedAt: header.receivedAt,
    before: memberOf(header),
    after: patched(memberOf(header), patch).state,
    restores: false,
  }));
  const members = new Map(threads.map((thread) => [thread.id, thread.members]));
  await land(store, accountId, {
    emails: { put: held.map((header) => Object.assign(header, served.get(header.id))) },
    threads: { put: threads.map((thread) => withServed(thread, served)) },
    queries: { put: await listsAfter(store, accountId, changes, members) },
  });
  return threadIds;
}

// One round of a job under the account's lock. The next emails it has
// left are asked in one request and written in a second that carries
// the mailbox changes behind the write; what the server took lands
// without a row in the log, the mailboxes after it. A round that finds
// nothing left ends the job; one the server takes nothing of fails it.
export async function jobRound(
  client: JmapClient,
  store: MailStore,
  progress: JobProgress,
): Promise<JobRound> {
  const session = await client.session();
  const limit = Math.min(session.maxObjectsInSet, session.maxObjectsInGet);
  if (limit === 0) {
    throw new JmapError("limit", { limit: "maxObjectsInSet" });
  }
  const reached = await reach(client, progress, limit);
  const total = progress.total ?? reached.total;
  if (reached.ids.length === 0) {
    return { progress: { ...progress, total, state: "done" }, changes: nothing() };
  }
  const { job } = progress;
  const mailboxes = await mailboxPass(client, store);
  const responses = await client.request([
    callsFor(session.accountId).set("Email", { ...writeOf(job, reached), ifInState: null }, "s"),
    ...mailboxes.calls,
  ]);
  const { updated, destroyed } = acknowledged(responses);
  const taken = new Set([...updated, ...destroyed]);
  const last = reached.ids.find((id) => taken.has(id));
  if (last === undefined) {
    throw new MethodFailure("serverFail", "s");
  }
  const { accountId } = client;
  const gone = destroyed.length > 0 ? await goneOf(store, accountId, destroyed) : null;
  if (gone !== null) {
    await land(store, accountId, gone.batch);
  }
  const threads = await landUpdated(store, accountId, updated, patchOf(job));
  await mailboxes.land(responses);
  return {
    progress: { ...progress, done: progress.done + taken.size, total, last },
    changes: {
      mailboxes: true,
      windows: [job.mailboxId],
      threads: [...new Set([...(gone?.changes.threads ?? []), ...threads])],
    },
  };
}

// What a run of a job asks of its host: one round under the account's
// lock, whether the user canceled it and a word to the tabs after each
// round.
export interface JobControls {
  round(progress: JobProgress): Promise<JobRound>;
  canceled(): boolean;
  told(round: JobRound): void;
}

export function jobStarted(job: Job): JobProgress {
  return { job, done: 0, total: null, last: null, state: "running" };
}

// The job ended as failed, with what the rounds before did kept.
function failedRound(progress: JobProgress): JobRound {
  return { progress: { ...progress, state: "failed" }, changes: nothing() };
}

// Round after round until the job is done; a cancel ends it after the
// round that runs. A round that throws ends the job as failed: the tabs
// hear that with what the rounds before it did kept and the error goes
// on to the caller.
export async function runJob(progress: JobProgress, controls: JobControls): Promise<JobProgress> {
  const round = await controls.round(progress).catch((error: unknown) => {
    controls.told(failedRound(progress));
    throw error;
  });
  const after: JobRound =
    round.progress.state === "running" && controls.canceled()
      ? { ...round, progress: { ...round.progress, state: "canceled" } }
      : round;
  controls.told(after);
  return after.progress.state === "running" ? runJob(after.progress, controls) : after.progress;
}
