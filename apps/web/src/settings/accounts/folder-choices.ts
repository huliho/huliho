// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { treeOrder, visibleMailboxes } from "@huliho/core";
import type { Mailbox, MailboxWrite } from "@huliho/core";

import { moveTargetsOf, pathOf } from "../../mail/actions/move-mode";

// The six roles in the order the tree draws them.
export const ROLES = ["inbox", "drafts", "sent", "archive", "junk", "trash"] as const;
export type RoleName = (typeof ROLES)[number];

// How a nested folder reads in a list with no room for its depth.
const PATH_SEPARATOR = " / ";

// One mailbox as a choice names it.
export interface FolderChoice {
  id: string;
  label: string;
}

// One role as its row draws it: the mailbox holding it, whether the server
// keeps it there and the mailboxes it can go to.
export interface RoleRow {
  role: RoleName;
  holderId: string | null;
  fixed: boolean;
  choices: FolderChoice[];
}

// A mailbox the server does not open, with whether the tree leaves it out.
export interface ClosedFolder extends FolderChoice {
  hidden: boolean;
}

function labelOf(names: readonly string[], name: string): string {
  return [...names, name].join(PATH_SEPARATOR);
}

// A mailbox that takes no rename stands where the server fixes it, as INBOX
// does and a label Gmail keeps for itself: it keeps the role it holds and
// takes no other.
function fixedInPlace(mailbox: Mailbox): boolean {
  return !mailbox.myRights.mayRename;
}

// Every role with what its select offers: the mailboxes the tree draws
// that can hold mail and move, in the tree's order. A mailbox fixed in
// place stands on the row of the role it holds alone.
export function roleRows(mailboxes: readonly Mailbox[], locale: string): RoleRow[] {
  const takers = mailboxes.filter((row) => row.myRights.mayReadItems && !fixedInPlace(row));
  const offered = new Set(takers.map(({ id }) => id));
  return ROLES.map((role) => {
    const holder = mailboxes.find((row) => row.role === role);
    const ids = holder === undefined ? offered : new Set([...offered, holder.id]);
    return {
      role,
      holderId: holder?.id ?? null,
      fixed: holder !== undefined && fixedInPlace(holder),
      choices: moveTargetsOf([...ids], mailboxes, locale).map((target) => ({
        id: target.id,
        label: labelOf(target.path, target.name),
      })),
    };
  });
}

// The write that gives a role to `takerId`, or to no mailbox, null when
// nothing moves. It names the holder letting go before the taker, and a
// server settles on the call's final state whichever of the two comes first.
export function roleWrite(
  mailboxes: readonly Mailbox[],
  role: RoleName,
  takerId: string | null,
): MailboxWrite | null {
  const holderId = mailboxes.find((row) => row.role === role)?.id ?? null;
  if (holderId === takerId) {
    return null;
  }
  return {
    update: {
      ...(holderId === null ? {} : { [holderId]: { role: null } }),
      ...(takerId === null ? {} : { [takerId]: { role } }),
    },
  };
}

// The write that brings one folder back; its subfolders stay as they are.
export function showWrite(id: string): MailboxWrite {
  return { update: { [id]: { isSubscribed: true } } };
}

function inTreeOrder(mailboxes: readonly Mailbox[], locale: string): Mailbox[] {
  const byId = new Map(mailboxes.map((row) => [row.id, row]));
  return treeOrder(mailboxes, locale).flatMap((id) => byId.get(id) ?? []);
}

// The mailboxes the tree leaves out, in the tree's order.
export function hiddenFolders(mailboxes: readonly Mailbox[], locale: string): FolderChoice[] {
  const drawn = new Set(visibleMailboxes(mailboxes).map(({ id }) => id));
  const byId = new Map(mailboxes.map((row) => [row.id, row]));
  return inTreeOrder(mailboxes, locale)
    .filter((row) => !drawn.has(row.id))
    .map((row) => ({ id: row.id, label: labelOf(pathOf(row, byId), row.name) }));
}

// The mailboxes the server does not open, each with the choice between
// the tree's empty row and no row. An account that subscribes nothing
// draws every mailbox whatever a choice says, so it offers none.
export function closedFolders(mailboxes: readonly Mailbox[], locale: string): ClosedFolder[] {
  if (!mailboxes.some((row) => row.isSubscribed)) {
    return [];
  }
  const drawn = new Set(visibleMailboxes(mailboxes).map(({ id }) => id));
  const byId = new Map(mailboxes.map((row) => [row.id, row]));
  return inTreeOrder(mailboxes, locale)
    .filter((row) => !row.myRights.mayReadItems && row.role === null)
    .map((row) => ({
      id: row.id,
      label: labelOf(pathOf(row, byId), row.name),
      hidden: !drawn.has(row.id),
    }));
}
