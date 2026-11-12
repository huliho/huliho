// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { DraftContent, DraftRow, MailStore } from "./store";

// How long a queued draft waits before the drain sends it: the window
// in which Undo takes the send back.
export const SEND_UNDO_MS = 10_000;

// The server's draft a row was opened from, which its first write
// replaces.
export interface OpenedDraft {
  serverId: string;
  threadId: string | null;
}

// One write of the composer: the row's id, what it holds now and, for a
// row the store does not hold yet, the server's draft it was opened from.
export interface DraftSave {
  id: string;
  content: DraftContent;
  opened?: OpenedDraft;
}

// Whether the row holds a change the server has not taken yet.
export function needsWrite(row: DraftRow): boolean {
  return row.savedRevision !== row.revision;
}

function freshRow(save: DraftSave): DraftRow {
  return {
    ...save.content,
    id: save.id,
    serverId: save.opened?.serverId ?? null,
    threadId: save.opened?.threadId ?? null,
    parts: {},
    state: "draft",
    sendAfter: null,
    changedAt: 0,
    revision: 0,
    savedRevision: null,
    savedAt: null,
    failure: null,
  };
}

// Writes the composer's change into the row, run under the account's
// lock: the content replaced, the revision counted up, where the row
// stands on the server and in the queue kept.
export async function saveDraft(
  store: MailStore,
  accountId: string,
  save: DraftSave,
): Promise<DraftRow> {
  const held = (await store.draft(accountId, save.id)) ?? freshRow(save);
  const row: DraftRow = {
    ...held,
    ...save.content,
    revision: held.revision + 1,
    changedAt: Date.now(),
  };
  await store.commit(accountId, { drafts: { put: [row] } });
  return row;
}

// Send: the draft waits in the queue for SEND_UNDO_MS, a failure of an
// earlier send cleared. Null for a row that is gone or already queued.
export async function queueDraft(
  store: MailStore,
  accountId: string,
  id: string,
): Promise<DraftRow | null> {
  const row = await store.draft(accountId, id);
  if (row?.state !== "draft") {
    return null;
  }
  const queued: DraftRow = {
    ...row,
    state: "queued",
    sendAfter: Date.now() + SEND_UNDO_MS,
    failure: null,
  };
  await store.commit(accountId, { drafts: { put: [queued] } });
  return queued;
}

// Undo of a send inside its window: the row is a draft again. Null once
// the submission is on its way or the row is gone.
export async function undoSend(
  store: MailStore,
  accountId: string,
  id: string,
): Promise<DraftRow | null> {
  const row = await store.draft(accountId, id);
  if (row?.state !== "queued") {
    return null;
  }
  const draft: DraftRow = { ...row, state: "draft", sendAfter: null };
  await store.commit(accountId, { drafts: { put: [draft] } });
  return draft;
}
