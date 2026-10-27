// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AfterAction, ListRow, ThreadDetail } from "@huliho/core";

import { indexOfThread, rowAt } from "../use-thread-pages";
import type { Aim, PlaceOf } from "./taken";
import { targetOfDetail, targetOfRow } from "./thread-target";

// A row as an action takes it, from the mailbox the row acts from.
export function aimAt(row: ListRow, index: number, placeOf: PlaceOf): Aim | null {
  const place = placeOf(row.mailboxId);
  return place === null ? null : { target: targetOfRow(row), index, place };
}

interface Where {
  rows: ReadonlyMap<number, ListRow[]>;
  cursor: number;
  openThreadId: string | null;
  // Whether the focus stands in the list's grid.
  inList: boolean;
  // The open thread as the pane read it, where it did.
  detail: ThreadDetail | null;
  // The mailbox the open thread acts from where no page holds its row.
  openMailboxId: string | null;
  placeOf: PlaceOf;
}

// The open thread whose row no page holds, through its detail.
function detailAim({ detail, openMailboxId, placeOf }: Where): Aim | null {
  const place = openMailboxId === null ? null : placeOf(openMailboxId);
  const target =
    detail === null || place === null ? null : targetOfDetail(detail, place.mailbox.id);
  return target === null || place === null ? null : { target, index: null, place };
}

// The thread an action takes: the open one unless the focus stands in
// the list, where the cursor's row is; the cursor's row while nothing is
// open.
export function aimOf(where: Where): Aim | null {
  const { rows, cursor, openThreadId, inList, placeOf } = where;
  if (openThreadId !== null && !inList) {
    const index = indexOfThread(rows, openThreadId);
    const row = index === null ? undefined : rowAt(rows, index);
    if (row !== undefined && index !== null) {
      return aimAt(row, index, placeOf);
    }
    return detailAim(where);
  }
  const row = rowAt(rows, cursor);
  return row === undefined ? null : aimAt(row, cursor, placeOf);
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
