// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { createObject } from "../compose/object";
import { callsFor, outcome } from "../jmap/calls";
import type { Outcome } from "../jmap/calls";
import { JmapError, SERVER_UNAVAILABLE } from "../jmap/client";
import type { JmapClient, MethodCall } from "../jmap/client";
import { emailSetAnswerSchema } from "../jmap/schemas";
import type { EmailSetAnswer, Invocation } from "../jmap/schemas";
import { partsOf, withParts } from "./draft-parts";
import { needsWrite } from "./drafts";
import { setMailbox } from "./mailbox-set";
import { syncMailboxes } from "./mailboxes";
import { applyChanges } from "./poll";
import type { AppliedChanges } from "./poll";
import type { DraftRow, MailStore } from "./store";

// The call id of a write inside the request it leads, and the creation
// id of the draft it creates.
const SET_CALL = "d";
const DRAFT_KEY = "draft";
// A destroy of an email the server does not hold has nothing left to do.
const NOT_FOUND = "notFound";
// The SetError of a draft whose attachments pass what the account takes.
const TOO_LARGE = "tooLarge";

// The names a Drafts or a Sent mailbox takes where the account lacks one,
// in the user's language.
export interface RoleNames {
  drafts: string;
  sent: string;
}

// What a write of the account's drafts needs besides the row: the lists
// the tabs watch and the names of a missing mailbox.
export interface DraftAsk {
  watched: readonly string[];
  names: RoleNames;
}

// What a write did: nothing to write, the draft taken by the server or
// the draft refused with its cause now in the row.
export type SaveOutcome =
  | { status: "clean" }
  | { status: "saved"; row: DraftRow; changes: AppliedChanges }
  | { status: "refused"; row: DraftRow; changes: AppliedChanges };

export const NO_CHANGES: AppliedChanges = { mailboxes: false, windows: [], threads: [] };

export function joined(one: AppliedChanges, other: AppliedChanges): AppliedChanges {
  return {
    mailboxes: one.mailboxes || other.mailboxes,
    windows: [...new Set([...one.windows, ...other.windows])],
    threads: [...new Set([...one.threads, ...other.threads])],
  };
}

// The calls in front of a poll's request, so what the server wrote lands
// in the store before the call resolves: their responses and what the
// poll changed.
export async function leading(
  client: JmapClient,
  store: MailStore,
  calls: readonly MethodCall[],
  watched: readonly string[],
): Promise<{ responses: readonly Invocation[]; changes: AppliedChanges }> {
  const led: (readonly Invocation[])[] = [];
  const changes = await applyChanges(client, store, watched, {
    calls,
    read: (responses) => {
      led.push(responses);
    },
  });
  return { responses: led.at(0) ?? [], changes };
}

// The account's mailbox with the role, created where none stands (RFC
// 8621 section 2); the SetError type answers in its place where the
// server refused the create.
export async function roleMailbox(
  client: JmapClient,
  store: MailStore,
  role: keyof RoleNames,
  ask: DraftAsk,
): Promise<{ id: string; changes: AppliedChanges } | { refused: string; changes: AppliedChanges }> {
  if ((await store.state(client.accountId, "Mailbox")) === null) {
    await syncMailboxes(client, store);
  }
  const held = (await store.mailboxes(client.accountId)).find((box) => box.role === role);
  if (held !== undefined) {
    return { id: held.id, changes: NO_CHANGES };
  }
  const name = role === "drafts" ? ask.names.drafts : ask.names.sent;
  const create = { name, parentId: null, role, isSubscribed: true };
  const written = await setMailbox(client, store, { create: { [role]: create } }, ask.watched);
  const id = new Map(Object.entries(written.created)).get(role);
  const refusal = new Map(Object.entries(written.notCreated)).get(role);
  return id === undefined
    ? { refused: refusal?.type ?? "serverFail", changes: written.changes }
    : { id, changes: written.changes };
}

// The row with the cause of a refused write, kept for the user.
async function refusedWith(
  store: MailStore,
  accountId: string,
  row: DraftRow,
  refusal: { type: string; changes: AppliedChanges },
): Promise<SaveOutcome> {
  const kept: DraftRow = { ...row, failure: { stage: "save", type: refusal.type, recipients: [] } };
  await store.commit(accountId, { drafts: { put: [kept] } });
  return { status: "refused", row: kept, changes: refusal.changes };
}

// The SetError type that refused the draft's create, if one did.
function refusalOf(read: Outcome<EmailSetAnswer>): string | undefined {
  return read.status === "error"
    ? read.type
    : new Map(Object.entries(read.value.notCreated ?? {})).get(DRAFT_KEY)?.type;
}

// The answer to the draft's create: its id and thread, or the SetError
// type in its place with whether the copy it replaces is gone, as a
// server that runs the two apart leaves it (RFC 8620 section 5.3). A
// server out of reach throws, so the row waits.
function createdOf(
  read: Outcome<EmailSetAnswer>,
  replaced: string | null,
): { id: string; threadId: string } | { refused: string; gone: boolean } {
  const answer = read.status === "ok" ? read.value : null;
  const made = new Map(Object.entries(answer?.created ?? {})).get(DRAFT_KEY);
  if (made !== undefined) {
    return { id: made.id, threadId: made.threadId };
  }
  const type = refusalOf(read);
  if (type === SERVER_UNAVAILABLE) {
    throw new JmapError("unavailable");
  }
  const gone = replaced !== null && (answer?.destroyed ?? []).includes(replaced);
  return { refused: type ?? "serverFail", gone };
}

// What the attachments of one email weigh together.
function weightOf(row: DraftRow): number {
  return row.attachments.reduce((total, file) => total + file.size, 0);
}

// The Email/set of one write, in front of a poll's request: the create
// with the files named by the server's parts, and the destroy of the copy
// it replaces.
async function sent(
  client: JmapClient,
  store: MailStore,
  row: DraftRow,
  ask: DraftAsk & { draftsId: string },
): Promise<{ read: Outcome<EmailSetAnswer>; changes: AppliedChanges }> {
  const session = await client.session();
  const object = createObject(withParts(row), {
    draftsId: ask.draftsId,
    flowedText: session.flowedText,
  });
  const replaced = row.serverId === null ? {} : { destroy: [row.serverId] };
  const set = callsFor(session.accountId).set(
    "Email",
    { create: { [DRAFT_KEY]: object }, ...replaced, ifInState: null },
    SET_CALL,
  );
  const led = await leading(client, store, [set], ask.watched);
  return { read: outcome(led.responses, SET_CALL, emailSetAnswerSchema), changes: led.changes };
}

// The row once the server took it: the new copy's id and thread, the
// revision it carried, then the blobs of its parts.
async function saved(
  client: JmapClient,
  store: MailStore,
  row: DraftRow,
  made: { id: string; threadId: string },
): Promise<DraftRow> {
  const written: DraftRow = {
    ...row,
    serverId: made.id,
    threadId: made.threadId,
    parts: {},
    savedRevision: row.revision,
    savedAt: Date.now(),
    failure: row.failure?.stage === "save" ? null : row.failure,
  };
  await store.commit(client.accountId, { drafts: { put: [written] } });
  const parts = await partsOf(client, row, made.id);
  if (Object.keys(parts).length === 0) {
    return written;
  }
  const kept = { ...written, parts };
  await store.commit(client.accountId, { drafts: { put: [kept] } });
  return kept;
}

// Writes the row to the server under the account's lock: one Email/set
// that creates the draft in Drafts and destroys the copy it replaces, in
// front of a poll's request (RFC 8621 section 4.6). A new copy has a new
// id, which the row keeps with the revision it carried and the blobs of
// its parts; a refusal keeps the row with its cause. Attachments past
// the account's `maxSizeAttachmentsPerEmail` are refused before any
// request. A row without a change sends nothing; a request that does not
// reach the server throws.
export async function writeDraft(
  client: JmapClient,
  store: MailStore,
  id: string,
  ask: DraftAsk,
): Promise<SaveOutcome> {
  const { accountId } = client;
  const row = await store.draft(accountId, id);
  if (row === null || !needsWrite(row)) {
    return { status: "clean" };
  }
  const limit = (await client.session()).uploadLimits.maxSizeAttachmentsPerEmail;
  if (limit !== null && weightOf(row) > limit) {
    return refusedWith(store, accountId, row, { type: TOO_LARGE, changes: NO_CHANGES });
  }
  const drafts = await roleMailbox(client, store, "drafts", ask);
  if ("refused" in drafts) {
    return refusedWith(store, accountId, row, { type: drafts.refused, changes: drafts.changes });
  }
  const set = await sent(client, store, row, { ...ask, draftsId: drafts.id });
  const changes = joined(drafts.changes, set.changes);
  const made = createdOf(set.read, row.serverId);
  if ("refused" in made) {
    const left = made.gone ? { ...row, serverId: null, threadId: null, parts: {} } : row;
    return refusedWith(store, accountId, left, { type: made.refused, changes });
  }
  return { status: "saved", row: await saved(client, store, row, made), changes };
}

// What a discard did: the row gone and what it held, which Undo saves
// again. A refusal of the server keeps the row.
export type DiscardOutcome =
  | { status: "discarded"; row: DraftRow; changes: AppliedChanges }
  | { status: "refused"; type: string; changes: AppliedChanges };

// The server's copy destroyed in front of a poll's request, then the row;
// a draft never written leaves the server alone. A request that does not
// reach the server throws and keeps both.
export async function discardDraft(
  client: JmapClient,
  store: MailStore,
  id: string,
  watched: readonly string[],
): Promise<DiscardOutcome | null> {
  const { accountId } = client;
  const row = await store.draft(accountId, id);
  if (row === null) {
    return null;
  }
  let changes = NO_CHANGES;
  if (row.serverId !== null) {
    const calls = callsFor((await client.session()).accountId);
    const destroy = calls.set("Email", { destroy: [row.serverId], ifInState: null }, SET_CALL);
    const led = await leading(client, store, [destroy], watched);
    const read = outcome(led.responses, SET_CALL, emailSetAnswerSchema);
    const type =
      read.status === "error"
        ? read.type
        : new Map(Object.entries(read.value.notDestroyed ?? {})).get(row.serverId)?.type;
    if (type === SERVER_UNAVAILABLE) {
      throw new JmapError("unavailable");
    }
    if (type !== undefined && type !== NOT_FOUND) {
      return { status: "refused", type, changes: led.changes };
    }
    changes = led.changes;
  }
  await store.commit(accountId, { drafts: { remove: [id] } });
  return { status: "discarded", row, changes };
}
