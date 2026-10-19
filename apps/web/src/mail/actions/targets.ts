// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { actionFor } from "@huliho/core";
import type {
  ActionTarget,
  AfterAction,
  ListRow,
  MailAction,
  Mailbox,
  ThreadDetail,
} from "@huliho/core";

import { indexOfThread, rowAt } from "../use-thread-pages";
import type { ActionKind } from "./kinds";
import { targetOfDetail, targetOfRow } from "./thread-target";

// The thread an action takes and where its row stands in the list, null
// for a thread opened from elsewhere whose row no page holds.
export interface Aim {
  target: ActionTarget;
  index: number | null;
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

// Several threads as one for the words of a flip: unread while any is,
// starred while every one is.
export function combinedOf(aims: readonly Aim[]): Pick<ActionTarget, "unread" | "flagged"> {
  return {
    unread: aims.some(({ target }) => target.unread),
    flagged: aims.length > 0 && aims.every(({ target }) => target.flagged),
  };
}

// The action a key takes on several threads and the threads it changes:
// a move takes every one, a flip the ones not already on its side.
export function takenBy(
  kind: ActionKind,
  mailbox: Mailbox,
  aims: readonly Aim[],
): { action: MailAction; taken: Aim[] } {
  const action = actionFor(kind, mailbox, combinedOf(aims));
  return {
    action,
    taken: aims.filter(({ target }) => actionFor(kind, mailbox, target) === action),
  };
}

// The row that takes the place of the row at `index` once the rows at
// `leaving` go: the first loaded one after it that stays, else the
// last one before it that stays; null when none stays.
export function survivorOf(
  rows: ReadonlyMap<number, ListRow[]>,
  index: number,
  leaving: ReadonlySet<number>,
): { row: ListRow | undefined; index: number; after: boolean } | null {
  for (let at = index + 1; rowAt(rows, at) !== undefined; at += 1) {
    if (!leaving.has(at)) {
      return { row: rowAt(rows, at), index: at, after: true };
    }
  }
  for (let at = index - 1; at >= 0; at -= 1) {
    if (!leaving.has(at)) {
      return { row: rowAt(rows, at), index: at, after: false };
    }
  }
  return null;
}

// What follows an action that took the open thread out of the list:
// under `next` the row that takes its place opens, at the end of the
// list or under `list` the thread closes with the cursor on the row
// that takes its place, else on the row before it. Rows that leave
// with it, as a bulk action takes them, are passed over.
export type After = { open: ListRow; index: number } | { close: true; index: number | null };

export function afterOf(
  rows: ReadonlyMap<number, ListRow[]>,
  index: number | null,
  rule: AfterAction,
  leaving: ReadonlySet<number> = new Set(index === null ? [] : [index]),
): After {
  const survivor = index === null ? null : survivorOf(rows, index, leaving);
  if (survivor?.after === true && survivor.row !== undefined) {
    return rule === "next"
      ? { open: survivor.row, index: survivor.index }
      : { close: true, index: survivor.index };
  }
  return { close: true, index: survivor?.index ?? null };
}
