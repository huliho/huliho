// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow, Mailbox } from "@huliho/core";

import { INBOX_PAGE, MAILBOXES } from "../fixtures";

// A mailbox that takes and gives up messages, as on a server that moves them.
function writable(box: Mailbox): Mailbox {
  return { ...box, myRights: { ...box.myRights, mayAddItems: true, mayRemoveItems: true } };
}

// The fixtures' tree on a server that moves messages.
export const WRITABLE: Mailbox[] = MAILBOXES.map(writable);

export function boxOf(role: string): Mailbox {
  const found = WRITABLE.find((box) => box.role === role);
  if (found === undefined) {
    throw new Error(`no ${role} in the fixtures`);
  }
  return found;
}

export function rowAt(index: number): ListRow {
  const found = INBOX_PAGE.rows.at(index);
  if (found === undefined) {
    throw new Error(`the fixture page holds no row ${String(index)}`);
  }
  return found;
}
