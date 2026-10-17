// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ActionTarget, AfterAction, ListRow, ThreadDetail } from "@huliho/core";

import { indexOfThread, rowAt } from "../use-thread-pages";

const SEEN = "$seen";
const FLAGGED = "$flagged";

// The thread an action takes and where its row stands in the list, null
// for a thread opened from elsewhere whose row no page holds.
export interface Aim {
  target: ActionTarget;
  index: number | null;
}

function targetOfRow(row: ListRow): ActionTarget {
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

interface Where {
  rows: ReadonlyMap<number, ListRow[]>;
  cursor: number;
  openThreadId: string | null;
  // Whether the focus stands in the list's grid.
  inList: boolean;
  // The open thread as the pane read it, where it did.
  detail: ThreadDetail | null;
  mailboxId: string;
}

// The thread an action takes: the open one unless the focus stands in
// the list, where the cursor's row is; the cursor's row while nothing is
// open.
export function aimOf({
  rows,
  cursor,
  openThreadId,
  inList,
  detail,
  mailboxId,
}: Where): Aim | null {
  if (openThreadId !== null && !inList) {
    const index = indexOfThread(rows, openThreadId);
    const row = index === null ? undefined : rowAt(rows, index);
    if (row !== undefined) {
      return { target: targetOfRow(row), index };
    }
    const target = detail === null ? null : targetOfDetail(detail, mailboxId);
    return target === null ? null : { target, index: null };
  }
  const row = rowAt(rows, cursor);
  return row === undefined ? null : { target: targetOfRow(row), index: cursor };
}

// What follows an action that took the open thread out of the list:
// under `next` the row that takes its place opens, at the end of the
// list or under `list` the thread closes with the cursor on the row
// that takes its place, else on the row before it.
export type After = { open: ListRow; index: number } | { close: true; index: number | null };

export function afterOf(
  rows: ReadonlyMap<number, ListRow[]>,
  index: number | null,
  rule: AfterAction,
): After {
  const next = index === null ? undefined : rowAt(rows, index + 1);
  if (index !== null && next !== undefined) {
    return rule === "next" ? { open: next, index: index + 1 } : { close: true, index: index + 1 };
  }
  return { close: true, index: index !== null && index > 0 ? index - 1 : null };
}
