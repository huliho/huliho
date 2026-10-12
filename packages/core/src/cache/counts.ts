// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "../jmap/schemas";
import type { MemberState } from "./store";

// The keyword of a message that was read (RFC 8621 section 4.1.1).
export const SEEN = "$seen";

type Members = Record<string, MemberState>;

// How far the four counts of one mailbox move (RFC 8621 section 2).
type Move = Pick<Mailbox, "totalEmails" | "unreadEmails" | "totalThreads" | "unreadThreads">;

export type Moves = Map<string, Move>;

function still(): Move {
  return { totalEmails: 0, unreadEmails: 0, totalThreads: 0, unreadThreads: 0 };
}

function moves(move: Move): boolean {
  return Object.values(move).some((count) => count !== 0);
}

// The emails of a thread in a mailbox and the unread ones among them.
function countIn(members: Members, mailboxId: string): { all: number; unread: number } {
  const inside = Object.values(members).filter((member) => mailboxId in member.mailboxIds);
  return {
    all: inside.length,
    unread: inside.filter((member) => !(SEEN in member.keywords)).length,
  };
}

// How the counts of each mailbox move when the members of one thread go
// from `before` to `after`: an email per member that entered, left or
// changed, a thread when its first member entered or its last one left
// and the same for the unread ones.
function threadMoves(before: Members, after: Members): Moves {
  const mailboxIds = new Set(
    [...Object.values(before), ...Object.values(after)].flatMap((member) =>
      Object.keys(member.mailboxIds),
    ),
  );
  const found: Moves = new Map();
  for (const id of mailboxIds) {
    const was = countIn(before, id);
    const is = countIn(after, id);
    const move = {
      totalEmails: is.all - was.all,
      unreadEmails: is.unread - was.unread,
      totalThreads: Number(is.all > 0) - Number(was.all > 0),
      unreadThreads: Number(is.unread > 0) - Number(was.unread > 0),
    };
    if (moves(move)) {
      found.set(id, move);
    }
  }
  return found;
}

function combined(first: Move, second: Move, sign: number): Move {
  return {
    totalEmails: first.totalEmails + sign * second.totalEmails,
    unreadEmails: first.unreadEmails + sign * second.unreadEmails,
    totalThreads: first.totalThreads + sign * second.totalThreads,
    unreadThreads: first.unreadThreads + sign * second.unreadThreads,
  };
}

function addMoves(total: Moves, more: Moves): Moves {
  const sum: Moves = new Map(total);
  for (const [id, move] of more) {
    sum.set(id, combined(sum.get(id) ?? still(), move, 1));
  }
  return sum;
}

// What takes the counts from the moves `was` to the moves `now`.
export function shiftOf(was: Moves, now: Moves): Moves {
  const shift: Moves = new Map();
  for (const id of new Set([...was.keys(), ...now.keys()])) {
    const move = combined(now.get(id) ?? still(), was.get(id) ?? still(), -1);
    if (moves(move)) {
      shift.set(id, move);
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

// How the counts move when emails change inside their threads;
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
      threadMoves(membersWith(base, inThread, "before"), membersWith(base, inThread, "after")),
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
    const counted = combined(row, move, 1);
    return [
      {
        ...row,
        totalEmails: Math.max(0, counted.totalEmails),
        unreadEmails: Math.max(0, counted.unreadEmails),
        totalThreads: Math.max(0, counted.totalThreads),
        unreadThreads: Math.max(0, counted.unreadThreads),
      },
    ];
  });
}
