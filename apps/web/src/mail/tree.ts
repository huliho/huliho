// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";

// The six special-use roles in the order the tree draws them, each with
// the jump letter the keyboard map keeps for it in every locale.
const ROLE_LETTERS = new Map<string, string>([
  ["inbox", "i"],
  ["drafts", "d"],
  ["sent", "s"],
  ["archive", "a"],
  ["junk", "j"],
  ["trash", "t"],
]);

const LETTER = /[a-z]/u;

export interface TreeRow {
  mailbox: Mailbox;
  // The jump letter, null when the name has no free one.
  letter: string | null;
  children: TreeRow[];
}

// The tree as drawn: the roles first in their fixed order, then every
// other mailbox under "Folders", nested by parent.
export interface MailboxTreeModel {
  roles: TreeRow[];
  folders: TreeRow[];
}

// One row of a flat tree, with the place among its siblings that tells
// a screen reader the shape the DOM does not.
export interface FlatRow {
  mailbox: Mailbox;
  letter: string | null;
  level: number;
  position: number;
  size: number;
}

// The mailboxes as the query hands them to the shell.
export type TreeState =
  | { status: "pending" }
  | { status: "error"; retry: () => void }
  | { status: "success"; mailboxes: Mailbox[] };

// A drafts folder counts its drafts, since an unread draft is nothing to
// read; every other mailbox counts its unread mail.
export function countOf(mailbox: Mailbox): number {
  return mailbox.role === "drafts" ? mailbox.totalEmails : mailbox.unreadEmails;
}

function byOrder(collator: Intl.Collator): (first: Mailbox, second: Mailbox) => number {
  return (first, second) =>
    first.sortOrder - second.sortOrder || collator.compare(first.name, second.name);
}

// The first letter of the name nobody holds yet; the roles hold theirs
// whether or not the account has them.
function freeLetter(name: string, taken: Set<string>): string | null {
  for (const letter of name.toLowerCase()) {
    if (LETTER.test(letter) && !taken.has(letter)) {
      taken.add(letter);
      return letter;
    }
  }
  return null;
}

function nest(
  children: ReadonlyMap<string | null, Mailbox[]>,
  parentId: string | null,
  taken: Set<string>,
): TreeRow[] {
  return (children.get(parentId) ?? []).map((mailbox) => ({
    mailbox,
    letter: freeLetter(mailbox.name, taken),
    children: nest(children, mailbox.id, taken),
  }));
}

export function buildTree(mailboxes: readonly Mailbox[], locale: string): MailboxTreeModel {
  const sorted = mailboxes.toSorted(byOrder(new Intl.Collator(locale)));
  const roles: TreeRow[] = [];
  for (const [role, letter] of ROLE_LETTERS) {
    const mailbox = sorted.find((row) => row.role === role);
    if (mailbox !== undefined) {
      roles.push({ mailbox, letter, children: [] });
    }
  }
  const roleIds = new Set(roles.map((row) => row.mailbox.id));
  const folders = sorted.filter((row) => !roleIds.has(row.id));
  const folderIds = new Set(folders.map((row) => row.id));
  // A folder under a role or under a parent the tree lacks starts at the top.
  const children = Map.groupBy(folders, (row) =>
    row.parentId !== null && folderIds.has(row.parentId) ? row.parentId : null,
  );
  return { roles, folders: nest(children, null, new Set(ROLE_LETTERS.values())) };
}

// The rows in drawing order, parents before their children.
export function flatten(rows: readonly TreeRow[], level = 1): FlatRow[] {
  return rows.flatMap((row, index) => [
    { mailbox: row.mailbox, letter: row.letter, level, position: index + 1, size: rows.length },
    ...flatten(row.children, level + 1),
  ]);
}

// Where an account opens: its inbox, else the first mailbox the tree draws.
export function landingMailbox(model: MailboxTreeModel): Mailbox | null {
  const inbox = model.roles.find((row) => row.mailbox.role === "inbox");
  return (inbox ?? model.roles[0] ?? model.folders[0])?.mailbox ?? null;
}
