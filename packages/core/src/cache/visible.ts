// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "../jmap/schemas";
import type { MailboxWrite } from "./mailbox-set";

// The mailboxes the tree, the jumps, the move picker, the search's `in:`
// suggestions and the folder screen's choices draw: the subscribed ones
// and the ones with a role, every mailbox where the account subscribes
// none. A folder above a subscribed folder is drawn too, so the folder
// keeps its place, the parent a server names through CHILDINFO (RFC 5258
// section 3.5); a role is drawn on its own.
export function visibleMailboxes(mailboxes: readonly Mailbox[]): Mailbox[] {
  if (!mailboxes.some((mailbox) => mailbox.isSubscribed)) {
    return [...mailboxes];
  }
  const byId = new Map(mailboxes.map((mailbox) => [mailbox.id, mailbox]));
  const drawn = new Set(mailboxes.filter((mailbox) => mailbox.role !== null).map(({ id }) => id));
  // A role stands in `drawn` from the start, so its walk ends where it begins.
  for (const subscribed of mailboxes.filter((mailbox) => mailbox.isSubscribed)) {
    for (let at: Mailbox | undefined = subscribed; at !== undefined && !drawn.has(at.id);) {
      drawn.add(at.id);
      at = at.parentId === null ? undefined : byId.get(at.parentId);
    }
  }
  return mailboxes.filter((mailbox) => drawn.has(mailbox.id));
}

// The folder and every folder without a role under it: the branch a hide
// takes out of the tree as one. A role mailbox stays, since the tree draws
// it on its own.
export function branchOf(mailboxes: readonly Mailbox[], id: string): string[] {
  const ids = [id];
  for (const at of ids) {
    for (const row of mailboxes) {
      if (row.parentId === at && row.role === null && !ids.includes(row.id)) {
        ids.push(row.id);
      }
    }
  }
  return ids;
}

// The write that takes a folder out of the tree with the subscribed
// folders of its branch.
export function hideWrite(mailboxes: readonly Mailbox[], id: string): MailboxWrite {
  const branch = new Set(branchOf(mailboxes, id));
  const subscribed = mailboxes.filter((row) => row.isSubscribed && branch.has(row.id));
  return {
    update: Object.fromEntries(subscribed.map((row) => [row.id, { isSubscribed: false }])),
  };
}
