// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { actionFor, offeredActions } from "@huliho/core";
import type { ActionTarget, MailAction, Mailbox, Offered } from "@huliho/core";

import type { ActionKind } from "./kinds";

// The mailbox a thread acts from, with the actions it offers there.
export interface Place {
  mailbox: Mailbox;
  offered: Offered;
}

// The place of one of the account's mailboxes by id; null for a mailbox
// the tree does not hold.
export type PlaceOf = (mailboxId: string) => Place | null;

export function placesOf(mailboxes: readonly Mailbox[]): PlaceOf {
  return (mailboxId) => {
    const mailbox = mailboxes.find((row) => row.id === mailboxId);
    return mailbox === undefined ? null : { mailbox, offered: offeredActions(mailbox, mailboxes) };
  };
}

// The thread an action takes, the mailbox it acts from and where its row
// stands in the list, null for a thread opened from elsewhere whose row
// no page holds.
export interface Aim {
  target: ActionTarget;
  index: number | null;
  place: Place;
}

// Several threads as one for the words of a flip: unread while any is,
// starred while every one is.
export function combinedOf(aims: readonly Aim[]): Pick<ActionTarget, "unread" | "flagged"> {
  return {
    unread: aims.some(({ target }) => target.unread),
    flagged: aims.length > 0 && aims.every(({ target }) => target.flagged),
  };
}

const BINS: ReadonlySet<string | null> = new Set(["trash", "junk"]);

// Whether a mailbox's offer holds an action of `kind`.
const OFFERS: ReadonlyMap<ActionKind, (offered: Offered) => boolean> = new Map([
  ["archive", (offered) => offered.archive !== null],
  ["delete", (offered) => offered.delete !== null || offered.deleteForever],
  ["spam", (offered) => offered.spam !== null],
  ["read", (offered) => offered.seen],
  ["star", (offered) => offered.flag],
  ["move", (offered) => offered.move.length > 0],
]);

export function offers(offered: Offered, kind: ActionKind): boolean {
  return OFFERS.get(kind)?.(offered) === true;
}

// The action a key takes on several threads and the threads it changes:
// each whose mailbox offers that action, a move every one, a flip the
// ones not already on its side. The action follows the first thread
// outside Junk and Trash, so threads from several mailboxes are deleted
// forever or marked as not spam only while every one stands in those two.
// Null without a thread.
export function takenBy(
  kind: ActionKind,
  aims: readonly Aim[],
): { action: MailAction; taken: Aim[] } | null {
  const lead = aims.find(({ place }) => !BINS.has(place.mailbox.role)) ?? aims.at(0);
  if (lead === undefined) {
    return null;
  }
  const action = actionFor(kind, lead.place.mailbox, combinedOf(aims));
  return {
    action,
    taken: aims.filter(
      ({ target, place }) =>
        offers(place.offered, kind) && actionFor(kind, place.mailbox, target) === action,
    ),
  };
}
