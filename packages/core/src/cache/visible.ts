// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "../jmap/schemas";

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
