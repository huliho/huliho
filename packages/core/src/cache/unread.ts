// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "../jmap/schemas";
import type { MemberState } from "./store";

// The keyword of a message that was read (RFC 8621 section 4.1.1).
export const SEEN = "$seen";

type Members = Record<string, MemberState>;

// How far the unread counts of one mailbox move.
interface Move {
  emails: number;
  threads: number;
}

export type Moves = Map<string, Move>;

function unreadIn(members: Members, mailboxId: string): number {
  return Object.values(members).filter(
    (member) => mailboxId in member.mailboxIds && !(SEEN in member.keywords),
  ).length;
}

// How the unread counts of each mailbox move when the members of one
// thread go from `before` to `after`: an email per member that changed
// and a thread when the last unread member of a mailbox was read or
// the first one became unread.
function unreadMoves(before: Members, after: Members): Moves {
  const mailboxIds = new Set(
    [...Object.values(before), ...Object.values(after)].flatMap((member) =>
      Object.keys(member.mailboxIds),
    ),
  );
  const found: Moves = new Map();
  for (const id of mailboxIds) {
    const was = unreadIn(before, id);
    const is = unreadIn(after, id);
    if (was !== is) {
      found.set(id, { emails: is - was, threads: Number(is > 0) - Number(was > 0) });
    }
  }
  return found;
}

function addMoves(total: Moves, more: Moves): Moves {
  const sum: Moves = new Map(total);
  for (const [id, move] of more) {
    const held = sum.get(id) ?? { emails: 0, threads: 0 };
    sum.set(id, { emails: held.emails + move.emails, threads: held.threads + move.threads });
  }
  return sum;
}

// What takes the counts from the moves `was` to the moves `now`.
export function shiftOf(was: Moves, now: Moves): Moves {
  const still: Move = { emails: 0, threads: 0 };
  const shift: Moves = new Map();
  for (const id of new Set([...was.keys(), ...now.keys()])) {
    const from = was.get(id) ?? still;
    const to = now.get(id) ?? still;
    if (from.emails !== to.emails || from.threads !== to.threads) {
      shift.set(id, { emails: to.emails - from.emails, threads: to.threads - from.threads });
    }
  }
  return shift;
}

// One email whose state changes inside its thread.
export interface MemberChange {
  id: string;
  threadId: string;
  before: MemberState;
  after: MemberState;
}

function membersWith(
  base: Members,
  changes: readonly MemberChange[],
  side: "before" | "after",
): Members {
  return {
    ...base,
    ...Object.fromEntries(
      changes.map((change) => [change.id, side === "before" ? change.before : change.after]),
    ),
  };
}

// How the unread counts move when emails change inside their threads;
// `members` holds what each thread knows of its other emails.
export function movesOf(
  members: ReadonlyMap<string, Members>,
  changes: readonly MemberChange[],
): Moves {
  const threadIds = new Set(changes.map((change) => change.threadId));
  let total: Moves = new Map();
  for (const threadId of threadIds) {
    const inThread = changes.filter((change) => change.threadId === threadId);
    const base = members.get(threadId) ?? {};
    total = addMoves(
      total,
      unreadMoves(membersWith(base, inThread, "before"), membersWith(base, inThread, "after")),
    );
  }
  return total;
}

// The mailbox rows a move names, with it counted in; a count stops at
// zero.
export function recounted(mailboxes: readonly Mailbox[], moved: Moves): Mailbox[] {
  return mailboxes.flatMap((row) => {
    const move = moved.get(row.id);
    if (move === undefined) {
      return [];
    }
    return [
      {
        ...row,
        unreadEmails: Math.max(0, row.unreadEmails + move.emails),
        unreadThreads: Math.max(0, row.unreadThreads + move.threads),
      },
    ];
  });
}
