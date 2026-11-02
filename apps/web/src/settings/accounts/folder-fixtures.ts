// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";

// The rights of a mailbox the user owns and manages.
const OWNED: Mailbox["myRights"] = {
  mayReadItems: true,
  mayAddItems: true,
  mayRemoveItems: true,
  maySetSeen: true,
  maySetKeywords: true,
  mayCreateChild: true,
  mayRename: true,
  mayDelete: true,
  maySubmit: false,
};

const FOLDER_ORDER = 10;
const ROLE_PLACES = ["inbox", "drafts", "sent", "archive", "junk", "trash"];

interface Facts {
  parentId?: string;
  subscribed?: boolean;
  role?: string;
}

function box(id: string, name: string, facts: Facts = {}): Mailbox {
  const role = facts.role ?? null;
  return {
    id,
    name,
    parentId: facts.parentId ?? null,
    role,
    sortOrder: role === null ? FOLDER_ORDER : ROLE_PLACES.indexOf(role),
    totalEmails: 0,
    unreadEmails: 0,
    totalThreads: 0,
    unreadThreads: 0,
    myRights: OWNED,
    isSubscribed: facts.subscribed ?? true,
  };
}

export function folderBox(id: string, name: string, parentId?: string): Mailbox {
  return box(id, name, parentId === undefined ? {} : { parentId });
}

export function hiddenBox(id: string, name: string, parentId?: string): Mailbox {
  return box(id, name, { subscribed: false, ...(parentId === undefined ? {} : { parentId }) });
}

export function roleBox(
  id: string,
  name: string,
  role: string,
  facts: Facts & { fixed?: boolean } = {},
): Mailbox {
  const row = box(id, name, { ...facts, role });
  return facts.fixed === true ? { ...row, myRights: { ...OWNED, mayRename: false } } : row;
}

// A mailbox the server fixes in place, as INBOX is everywhere and a label
// Gmail keeps for itself is there.
export function fixedBox(id: string, name: string, role: string): Mailbox {
  return roleBox(id, name, role, { fixed: true });
}

// A mailbox the server does not open: it reads nothing and takes a child
// at most.
export function closedBox(id: string, name: string, facts: Facts = {}): Mailbox {
  return {
    ...box(id, name, { subscribed: false, ...facts }),
    myRights: {
      ...OWNED,
      mayReadItems: false,
      mayAddItems: false,
      mayRemoveItems: false,
      maySetSeen: false,
      maySetKeywords: false,
      mayRename: false,
      mayDelete: false,
    },
  };
}

// An account with five of the six roles and the inbox fixed in place, as
// a server without a Junk folder has it.
export const FOLDERS: Mailbox[] = [
  fixedBox("mb-inbox", "Inbox", "inbox"),
  roleBox("mb-drafts", "Drafts", "drafts"),
  roleBox("mb-sent", "Sent", "sent"),
  roleBox("mb-archive", "Archive", "archive"),
  roleBox("mb-trash", "Trash", "trash"),
  folderBox("mb-facturen", "Facturen"),
  folderBox("mb-verbouwing", "Verbouwing"),
  folderBox("mb-offertes", "Offertes", "mb-verbouwing"),
];
