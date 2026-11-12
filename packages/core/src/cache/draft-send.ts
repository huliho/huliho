// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { answer, callsFor, outcome } from "../jmap/calls";
import { JmapError, MethodFailure, SERVER_UNAVAILABLE } from "../jmap/client";
import type { JmapClient } from "../jmap/client";
import {
  STATE_PROPERTIES,
  emailStateSchema,
  getAnswerSchema,
  setAnswerSchema,
  submissionSetAnswerSchema,
} from "../jmap/schemas";
import type { Invocation } from "../jmap/schemas";
import { NO_CHANGES, joined, leading, roleMailbox, writeDraft } from "./draft-write";
import type { DraftAsk } from "./draft-write";
import { needsWrite } from "./drafts";
import { pathOf } from "./patch";
import { mutateMany } from "./pending";
import type { PatchMutation } from "./pending";
import type { AppliedChanges } from "./poll";
import { mayPatch } from "./rights";
import type { DraftFailure, DraftRow, MailStore } from "./store";

// The call ids of the send and of the read of a send whose answer was
// lost, and the creation id of the submission.
const SEND_CALL = "s";
const LANDED_CALL = "l";
const SEND_KEY = "send";
// The keyword a draft carries until its submission went out.
const DRAFT = "$draft";
// The SetError of an account that takes no send (RFC 8621 section 7.5).
const FORBIDDEN_TO_SEND = "forbiddenToSend";

const stateAnswerSchema = getAnswerSchema(emailStateSchema);

// What became of one queued draft: sent, filed in Sent or not; refused,
// the row a draft again with its cause; or waiting for the next round.
export type SendOutcome =
  | { id: string; status: "sent"; filed: boolean }
  | { id: string; status: "refused"; failure: DraftFailure }
  | { id: string; status: "waiting" };

// What a round of the queue did and what the tabs read again.
export interface Drained {
  outcomes: SendOutcome[];
  changes: AppliedChanges;
}

interface Step {
  outcome: SendOutcome;
  changes: AppliedChanges;
}

// What every step of a round works with.
interface Round {
  client: JmapClient;
  store: MailStore;
  ask: DraftAsk;
}

function isDue(row: DraftRow, now: number): boolean {
  return (
    row.state === "sending" ||
    (row.state === "queued" && row.sendAfter !== null && row.sendAfter <= now)
  );
}

// A failure that leaves the row for the next round: the network or a
// server out of reach.
function isWaiting(error: unknown): boolean {
  return (
    (error instanceof JmapError && error.code === "unavailable") ||
    (error instanceof MethodFailure && error.type === SERVER_UNAVAILABLE)
  );
}

// The row a draft again with the cause the server named.
async function refused(
  { client, store }: Round,
  row: DraftRow,
  failure: DraftFailure,
  changes: AppliedChanges,
): Promise<Step> {
  const draft: DraftRow = { ...row, state: "draft", sendAfter: null, failure };
  await store.commit(client.accountId, { drafts: { put: [draft] } });
  return { outcome: { id: row.id, status: "refused", failure }, changes };
}

// The row back in the queue for the next round.
async function waiting(
  { client, store }: Round,
  row: DraftRow,
  changes: AppliedChanges,
): Promise<Step> {
  await store.commit(client.accountId, { drafts: { put: [{ ...row, state: "queued" }] } });
  return { outcome: { id: row.id, status: "waiting" }, changes };
}

// A send that went out: the row leaves, and the parent takes $answered
// for a reply or $forwarded for a forward through the log as any keyword
// (RFC 8621 section 4.1.1).
async function sent(
  round: Round,
  row: DraftRow,
  filed: boolean,
  changes: AppliedChanges,
): Promise<Step> {
  const { client, store } = round;
  await store.commit(client.accountId, { drafts: { remove: [row.id] } });
  const done: Step = { outcome: { id: row.id, status: "sent", filed }, changes };
  if (row.parentId === null || row.kind === "new") {
    return done;
  }
  const keyword = row.kind === "reply" ? "$answered" : "$forwarded";
  const mutation: PatchMutation = {
    type: "Email",
    id: row.parentId,
    patch: { [pathOf("keywords", keyword)]: true },
  };
  if (!(await mayPatch(client, store, mutation))) {
    return done;
  }
  const logged = await mutateMany(store, client.accountId, [mutation]);
  return { ...done, changes: joined(changes, logged.changes) };
}

// Whether a submission whose answer was lost went out: the server's copy
// is gone or carries $draft no more (RFC 8621 section 7.5).
async function landed({ client }: Round, serverId: string): Promise<boolean> {
  const calls = callsFor((await client.session()).accountId);
  const responses = await client.request([
    calls.get("Email", [serverId], LANDED_CALL, STATE_PROPERTIES),
  ]);
  const [email] = answer(responses, LANDED_CALL, stateAnswerSchema).list;
  return email === undefined || !(DRAFT in email.keywords);
}

// Whether the implicit Email/set behind the submission filed the email,
// moving it out of Drafts into Sent.
function filedOf(responses: readonly Invocation[], emailId: string): boolean {
  const implicit = responses.find(([name, , id]) => name === "Email/set" && id === SEND_CALL);
  const read = implicit === undefined ? null : setAnswerSchema.safeParse(implicit[1]);
  return read?.success === true && !Object.hasOwn(read.data.notUpdated ?? {}, emailId);
}

// The draft written where it changed since its last save, or the step of
// a write the server refused: the row the write kept, which names no
// server copy where the server destroyed it (RFC 8620 section 5.3), a
// draft again with the cause.
async function written(round: Round, row: DraftRow): Promise<DraftRow | Step> {
  if (!needsWrite(row)) {
    return row;
  }
  const saved = await writeDraft(round.client, round.store, row.id, round.ask);
  if (saved.status === "saved") {
    return saved.row;
  }
  const unsaved = { stage: "save" as const, type: "serverFail", recipients: [] };
  if (saved.status === "clean") {
    return refused(round, row, unsaved, NO_CHANGES);
  }
  return refused(round, saved.row, saved.row.failure ?? unsaved, saved.changes);
}

// The cause of a refused submission with the addresses it names.
function failureOf(
  type: string,
  error: { invalidRecipients?: string[] | undefined } | undefined,
): DraftFailure {
  return { stage: "send", type, recipients: error?.invalidRecipients ?? [] };
}

// The submission of one written draft with the update that files it in
// Sent, in front of a poll's request.
async function submit(round: Round, row: DraftRow, serverId: string): Promise<Step> {
  const { client, store, ask } = round;
  const session = await client.session();
  if (!session.submission) {
    return refused(round, row, failureOf(FORBIDDEN_TO_SEND, undefined), NO_CHANGES);
  }
  const drafts = await roleMailbox(client, store, "drafts", ask);
  const sentBox = await roleMailbox(client, store, "sent", ask);
  const boxes = joined(drafts.changes, sentBox.changes);
  if ("refused" in drafts || "refused" in sentBox) {
    const type = "refused" in drafts ? drafts.refused : "refused" in sentBox ? sentBox.refused : "";
    return refused(round, row, failureOf(type, undefined), boxes);
  }
  await store.commit(client.accountId, { drafts: { put: [{ ...row, state: "sending" }] } });
  const submission = callsFor(session.accountId).submissionSet(
    {
      create: { [SEND_KEY]: { identityId: row.identity.id, emailId: serverId } },
      onSuccessUpdateEmail: {
        [`#${SEND_KEY}`]: {
          [pathOf("mailboxIds", drafts.id)]: null,
          [pathOf("mailboxIds", sentBox.id)]: true,
          [pathOf("keywords", DRAFT)]: null,
        },
      },
    },
    SEND_CALL,
  );
  const led = await leading(client, store, [submission], ask.watched);
  const changes = joined(boxes, led.changes);
  const read = outcome(led.responses, SEND_CALL, submissionSetAnswerSchema);
  const error =
    read.status === "error"
      ? { type: read.type }
      : new Map(Object.entries(read.value.notCreated ?? {})).get(SEND_KEY);
  if (error === undefined) {
    return sent(round, row, filedOf(led.responses, serverId), changes);
  }
  return error.type === SERVER_UNAVAILABLE
    ? waiting(round, row, changes)
    : refused(round, row, failureOf(error.type, error), changes);
}

// One queued draft: a send a lost answer hid is read from the server
// first, a changed row written, then the submission.
async function sendOne(round: Round, row: DraftRow): Promise<Step> {
  if (row.state === "sending" && row.serverId !== null && (await landed(round, row.serverId))) {
    return sent(round, row, true, NO_CHANGES);
  }
  const current = await written(round, row);
  if ("outcome" in current) {
    return current;
  }
  if (current.serverId === null) {
    return refused(round, current, failureOf("serverFail", undefined), NO_CHANGES);
  }
  return submit(round, current, current.serverId);
}

async function inTurn(round: Round, rows: readonly DraftRow[]): Promise<Drained> {
  const [row, ...rest] = rows;
  if (row === undefined) {
    return { outcomes: [], changes: NO_CHANGES };
  }
  let step: Step;
  try {
    step = await sendOne(round, row);
  } catch (error) {
    if (!isWaiting(error)) {
      throw error;
    }
    return { outcomes: [{ id: row.id, status: "waiting" }], changes: NO_CHANGES };
  }
  if (step.outcome.status === "waiting") {
    return { outcomes: [step.outcome], changes: step.changes };
  }
  const after = await inTurn(round, rest);
  return {
    outcomes: [step.outcome, ...after.outcomes],
    changes: joined(step.changes, after.changes),
  };
}

// One round of the account's queue under its lock: every draft past its
// window and every one whose send a closed tab left open, one at a time
// in the order they were sent (RFC 8621 section 7). A refusal turns the
// row back into a draft with its cause; the network or a server out of
// reach ends the round with the row still queued.
export async function drainDrafts(
  client: JmapClient,
  store: MailStore,
  ask: DraftAsk,
): Promise<Drained> {
  const now = Date.now();
  const due = (await store.drafts(client.accountId))
    .filter((row) => isDue(row, now))
    .toSorted((one, other) => (one.sendAfter ?? 0) - (other.sendAfter ?? 0));
  return inTurn({ client, store, ask }, due);
}
