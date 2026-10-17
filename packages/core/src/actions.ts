// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { SEEN } from "./cache/counts";
import { pathOf, targetOf } from "./cache/patch";
import type { Mutation } from "./cache/pending";
import type { EmailPatch, PendingRow } from "./cache/store";
import type { Mailbox } from "./jmap/schemas";

// What the user does to a thread. Delete forever stands where delete
// would in Trash and Junk, not spam where spam would in Junk; the read
// and star flips pick their side from the thread.
export type MailAction =
  | "archive"
  | "delete"
  | "deleteForever"
  | "spam"
  | "notSpam"
  | "read"
  | "unread"
  | "star"
  | "unstar";

// The actions a mailbox offers, each with the mailbox it moves to.
export interface Offered {
  archive: string | null;
  // Trash's id; null in Trash and Junk, where delete forever stands instead.
  delete: string | null;
  // Whether delete forever stands: Trash or Junk shown, giving up its emails.
  deleteForever: boolean;
  // Junk's id, or the inbox's in Junk, where the action is not spam.
  spam: string | null;
  // Whether spam and not spam write their keywords beside the move.
  junkKeywords: boolean;
  seen: boolean;
  flag: boolean;
}

// One thread as an action takes it: its emails in the mailbox shown in
// the thread's order, oldest first; whether any is unread or flagged.
export interface ActionTarget {
  threadId: string;
  memberIds: readonly string[];
  unread: boolean;
  flagged: boolean;
}

const FLAGGED = "$flagged";
// The keywords a client sets when the user reports spam or marks a
// message legitimate (RFC 8621 section 4.1.1).
const JUNK = "$junk";
const NOT_JUNK = "$notjunk";

const SEEN_PATH = pathOf("keywords", SEEN);
const FLAGGED_PATH = pathOf("keywords", FLAGGED);

function bin(shown: Mailbox): boolean {
  return shown.role === "trash" || shown.role === "junk";
}

// The mailbox of a role an email can enter, other than the one shown.
function roleBox(mailboxes: readonly Mailbox[], role: string, shown: Mailbox): string | null {
  const found = mailboxes.find((row) => row.role === role);
  return found !== undefined && found.id !== shown.id && found.myRights.mayAddItems
    ? found.id
    : null;
}

// Which actions a mailbox offers, by its role and the rights of the
// mailboxes involved, never by the kind of account (RFC 8621 section 2).
// A move needs mayRemoveItems where the email leaves and mayAddItems
// where it enters; an action that cannot run is absent.
export function offeredActions(shown: Mailbox, mailboxes: readonly Mailbox[]): Offered {
  const rights = shown.myRights;
  const leaves = rights.mayRemoveItems;
  const kept = leaves && !bin(shown);
  const spam = roleBox(mailboxes, shown.role === "junk" ? "inbox" : "junk", shown);
  return {
    archive: kept ? roleBox(mailboxes, "archive", shown) : null,
    delete: kept ? roleBox(mailboxes, "trash", shown) : null,
    deleteForever: leaves && bin(shown),
    spam: leaves ? spam : null,
    junkKeywords: rights.maySetKeywords,
    seen: rights.maySetSeen,
    flag: rights.maySetKeywords,
  };
}

function eachMember(thread: ActionTarget, patch: EmailPatch): Mutation[] {
  return thread.memberIds.map((id) => ({ type: "Email", id, threadId: thread.threadId, patch }));
}

function newest(thread: ActionTarget, patch: EmailPatch): Mutation[] {
  const id = thread.memberIds.at(-1);
  return id === undefined ? [] : [{ type: "Email", id, threadId: thread.threadId, patch }];
}

// The keywords spam or not spam writes, where the mailbox lets it.
function junkPatch(offered: Offered, junk: boolean): EmailPatch {
  if (!offered.junkKeywords) {
    return {};
  }
  return {
    [pathOf("keywords", JUNK)]: junk ? true : null,
    [pathOf("keywords", NOT_JUNK)]: junk ? null : true,
  };
}

interface Place {
  shown: Mailbox;
  offered: Offered;
}

// A move names the mailbox left and the one entered in one patch, so
// the email is never in no mailbox (RFC 8621 section 4.1.1).
function moving(
  thread: ActionTarget,
  shown: Mailbox,
  to: string | null,
  keywords: EmailPatch = {},
): Mutation[] {
  if (to === null) {
    return [];
  }
  const patch: EmailPatch = {
    [pathOf("mailboxIds", shown.id)]: null,
    [pathOf("mailboxIds", to)]: true,
  };
  return eachMember(thread, { ...patch, ...keywords });
}

function destroying(thread: ActionTarget): Mutation[] {
  return thread.memberIds.map((id) => ({
    type: "Email",
    id,
    threadId: thread.threadId,
    destroy: true,
  }));
}

// Each action's changes to one thread. A move takes every email in the
// mailbox shown; read marks every one read and unread the newest; star
// flags the newest and unstar clears every flag.
const BUILDERS: ReadonlyMap<MailAction, (thread: ActionTarget, place: Place) => Mutation[]> =
  new Map([
    ["archive", (thread, { shown, offered }) => moving(thread, shown, offered.archive)],
    ["delete", (thread, { shown, offered }) => moving(thread, shown, offered.delete)],
    ["deleteForever", (thread, { offered }) => (offered.deleteForever ? destroying(thread) : [])],
    [
      "spam",
      (thread, { shown, offered }) =>
        shown.role === "junk" ? [] : moving(thread, shown, offered.spam, junkPatch(offered, true)),
    ],
    [
      "notSpam",
      (thread, { shown, offered }) =>
        shown.role === "junk" ? moving(thread, shown, offered.spam, junkPatch(offered, false)) : [],
    ],
    [
      "read",
      (thread, { offered }) => (offered.seen ? eachMember(thread, { [SEEN_PATH]: true }) : []),
    ],
    [
      "unread",
      (thread, { offered }) => (offered.seen ? newest(thread, { [SEEN_PATH]: null }) : []),
    ],
    [
      "star",
      (thread, { offered }) => (offered.flag ? newest(thread, { [FLAGGED_PATH]: true }) : []),
    ],
    [
      "unstar",
      (thread, { offered }) => (offered.flag ? eachMember(thread, { [FLAGGED_PATH]: null }) : []),
    ],
  ]);

// The changes an action makes to one thread; empty where the mailbox
// does not offer it.
export function actionMutations(
  action: MailAction,
  shown: Mailbox,
  offered: Offered,
  thread: ActionTarget,
): Mutation[] {
  return BUILDERS.get(action)?.(thread, { shown, offered }) ?? [];
}

// The action a key takes on a thread in a mailbox: delete forever and
// not spam where the mailbox asks for them; the read and star flips by
// the thread's state.
export function actionFor(
  kind: "archive" | "delete" | "spam" | "read" | "star",
  shown: Mailbox,
  thread: Pick<ActionTarget, "unread" | "flagged">,
): MailAction {
  switch (kind) {
    case "delete":
      return bin(shown) ? "deleteForever" : "delete";
    case "spam":
      return shown.role === "junk" ? "notSpam" : "spam";
    case "read":
      return thread.unread ? "read" : "unread";
    case "star":
      return thread.flagged ? "unstar" : "star";
    default:
      return kind;
  }
}

// What a logged change did, in the words a refusal names it by.
export type ChangeVerb = MailAction | "move";

// The roles of the mailboxes a move entered and of those it left; null
// for a change that enters none.
function movedBetween(
  patch: EmailPatch,
  roles: ReadonlyMap<string, string | null>,
): { to: string | null; from: string[] } | null {
  const entered: string[] = [];
  const left: string[] = [];
  for (const [path, value] of Object.entries(patch)) {
    const { set, key } = targetOf(path);
    if (set === "mailboxIds") {
      (value === true ? entered : left).push(key);
    }
  }
  const to = entered[0];
  if (to === undefined) {
    return null;
  }
  return { to: roles.get(to) ?? null, from: left.map((id) => roles.get(id) ?? "") };
}

const ENTERED: ReadonlyMap<string, ChangeVerb> = new Map([
  ["archive", "archive"],
  ["trash", "delete"],
  ["junk", "spam"],
]);

// A mark by its keyword; anything else reads as a move.
function markVerb(patch: EmailPatch): ChangeVerb {
  const marks = new Map(Object.entries(patch));
  const flagged = marks.get(FLAGGED_PATH);
  if (flagged !== undefined) {
    return flagged === true ? "star" : "unstar";
  }
  const seen = marks.get(SEEN_PATH);
  if (seen !== undefined) {
    return seen === true ? "read" : "unread";
  }
  return "move";
}

// A refused change by what it did: a destroy deletes forever, a move
// reads by the role of the mailbox it entered and a mark by its keyword.
export function verbOf(
  row: Pick<PendingRow, "patch" | "destroy">,
  mailboxes: readonly Mailbox[],
): ChangeVerb {
  if (row.destroy === true) {
    return "deleteForever";
  }
  const moved = movedBetween(row.patch, new Map(mailboxes.map((box) => [box.id, box.role])));
  if (moved === null) {
    return markVerb(row.patch);
  }
  if (moved.to === "inbox" && moved.from.includes("junk")) {
    return "notSpam";
  }
  return ENTERED.get(moved.to ?? "") ?? "move";
}
