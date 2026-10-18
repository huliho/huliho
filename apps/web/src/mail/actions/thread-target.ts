// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ActionTarget, ListRow, ThreadDetail } from "@huliho/core";

const SEEN = "$seen";
const FLAGGED = "$flagged";

// A thread as its row in the list holds it.
export function targetOfRow(row: ListRow): ActionTarget {
  return {
    threadId: row.threadId,
    memberIds: row.memberIds,
    unread: row.unread,
    flagged: row.flagged,
  };
}

// The open thread as the reading pane holds it: its emails in the
// mailbox shown, in the thread's order.
export function targetOfDetail(detail: ThreadDetail, mailboxId: string): ActionTarget | null {
  const members = new Map(Object.entries(detail.thread.members));
  const memberIds = detail.thread.emailIds.filter(
    (id) => mailboxId in (members.get(id)?.mailboxIds ?? {}),
  );
  const keywords = memberIds.map((id) => members.get(id)?.keywords ?? {});
  if (memberIds.length === 0) {
    return null;
  }
  return {
    threadId: detail.thread.id,
    memberIds,
    unread: keywords.some((held) => !(SEEN in held)),
    flagged: keywords.some((held) => FLAGGED in held),
  };
}
