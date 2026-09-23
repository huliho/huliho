// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow, Mailbox } from "@huliho/core";

const HOUR_MS = 3_600_000;
const CREATED_AT = 1_778_750_400_000;

const READ_ONLY = {
  mayReadItems: true,
  mayAddItems: false,
  mayRemoveItems: false,
  maySetSeen: false,
  maySetKeywords: false,
  mayCreateChild: false,
  mayRename: false,
  mayDelete: false,
  maySubmit: false,
};

// id, name, role, sort order, total, unread and the parent, if any.
type Row = [string, string, string | null, number, number, number, string?];

const ROWS: Row[] = [
  ["mb-trash", "Trash", "trash", 5, 12, 0],
  ["mb-inbox", "Inbox", "inbox", 0, 1204, 23],
  ["mb-drafts", "Drafts", "drafts", 1, 2, 0],
  ["mb-sent", "Sent", "sent", 2, 310, 0],
  ["mb-archive", "Archive", "archive", 3, 4021, 0],
  ["mb-junk", "Junk", "junk", 4, 9, 1],
  ["mb-facturen", "Facturen", null, 10, 40, 3],
  ["mb-verbouwing", "Verbouwing", null, 10, 0, 0],
  ["mb-offertes", "Offertes", null, 10, 5, 0, "mb-verbouwing"],
];

function build(rows: readonly Row[]): Mailbox[] {
  return rows.map(([id, name, role, sortOrder, total, unread, parentId]) => ({
    id,
    name,
    parentId: parentId ?? null,
    role,
    sortOrder,
    totalEmails: total,
    unreadEmails: unread,
    totalThreads: total,
    unreadThreads: unread,
    myRights: READ_ONLY,
    isSubscribed: true,
  }));
}

function emptied([id, name, role, sortOrder, , , parentId]: Row): Row {
  return parentId === undefined
    ? [id, name, role, sortOrder, 0, 0]
    : [id, name, role, sortOrder, 0, 0, parentId];
}

// The six roles and three folders, one nested and one empty, listed out
// of order as a server may list them.
export const MAILBOXES: Mailbox[] = build(ROWS);

// The same tree with nothing in the inbox.
export const MAILBOXES_EMPTY_INBOX: Mailbox[] = build(
  ROWS.map((row) => (row[2] === "inbox" ? emptied(row) : row)),
);

export const FASTMAIL: AccountRow = {
  id: "acc-1",
  address: "sanne@fastmail.com",
  name: "Fastmail",
  provider: "fastmail",
  kind: "jmap",
  authMethod: "bearer",
  stoppedCause: null,
  stoppedAt: null,
  createdAt: CREATED_AT,
};

export const GMAIL: AccountRow = {
  id: "acc-2",
  address: "s.bakker@gmail.com",
  name: "Gmail",
  provider: "gmail",
  kind: "imap",
  authMethod: "password",
  stoppedCause: null,
  stoppedAt: null,
  createdAt: CREATED_AT + HOUR_MS,
};

export const ACCOUNTS: AccountRow[] = [FASTMAIL, GMAIL];
