// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow, Mailbox, ThreadDetail } from "@huliho/core";
import { expect, test } from "vitest";

import { INBOX_ID, MAILBOXES } from "../fixtures";
import { combinedOf, placesOf, takenBy } from "./taken";
import type { Aim } from "./taken";
import { afterOf, aimAt, aimOf, survivorOf } from "./targets";
import { targetOfDetail } from "./thread-target";

const WRITABLE: Mailbox[] = MAILBOXES.map((box) => ({
  ...box,
  myRights: { ...box.myRights, mayAddItems: true, mayRemoveItems: true },
}));
const PLACES = placesOf(WRITABLE);
const ARCHIVE = "mb-archive";
const TRASH = "mb-trash";
const JUNK = "mb-junk";
const SENT = "mb-sent";

function row(threadId: string, memberIds = [`${threadId}-1`], mailboxId = INBOX_ID): ListRow {
  return {
    id: memberIds[0] ?? threadId,
    threadId,
    mailboxId,
    sender: null,
    subject: null,
    preview: "",
    receivedAt: "2026-10-01T00:00:00Z",
    unread: false,
    flagged: false,
    hasAttachment: false,
    memberIds,
    count: memberIds.length,
  };
}

// The aim of a row, which every test's row has a place for.
function aim(one: ListRow, index = 0): Aim {
  const found = aimAt(one, index, PLACES);
  if (found === null) {
    throw new Error(`no place for ${one.mailboxId}`);
  }
  return found;
}

// The first page with t0, t1 and t2.
const ROWS = new Map([[0, [row("t0"), row("t1"), row("t2")]]]);

// The thread t9: e1 read in the inbox, e2 flagged in Sent, e3 unread in
// the inbox.
const DETAIL: ThreadDetail = {
  thread: {
    id: "t9",
    emailIds: ["e1", "e2", "e3"],
    members: {
      e1: { keywords: { $seen: true }, mailboxIds: { [INBOX_ID]: true } },
      e2: { keywords: { $seen: true, $flagged: true }, mailboxIds: { [SENT]: true } },
      e3: { keywords: {}, mailboxIds: { [INBOX_ID]: true } },
    },
  },
  emails: {},
};

const WHERE = {
  rows: ROWS,
  cursor: 2,
  openThreadId: null,
  inList: false,
  detail: null,
  openMailboxId: INBOX_ID,
  placeOf: PLACES,
};

test("the open thread takes its members in the mailbox it acts from, in the thread's order", () => {
  expect(targetOfDetail(DETAIL, INBOX_ID)).toEqual({
    threadId: "t9",
    memberIds: ["e1", "e3"],
    unread: true,
    flagged: false,
  });
  expect(targetOfDetail(DETAIL, SENT)).toEqual({
    threadId: "t9",
    memberIds: ["e2"],
    unread: false,
    flagged: true,
  });
  expect(targetOfDetail(DETAIL, ARCHIVE)).toBeNull();
});

test("a mailbox's place holds what it offers; one the tree lacks has none", () => {
  expect(PLACES(INBOX_ID)?.offered.archive).toBe(ARCHIVE);
  expect(PLACES(ARCHIVE)?.offered.archive).toBeNull();
  expect(PLACES("gone")).toBeNull();
});

test("the open thread is the aim unless the focus stands in the list", () => {
  expect(aimOf({ ...WHERE, openThreadId: "t1" })).toMatchObject({
    target: { threadId: "t1" },
    index: 1,
  });
  expect(aimOf({ ...WHERE, openThreadId: "t1", inList: true })).toMatchObject({
    target: { threadId: "t2" },
    index: 2,
  });
  expect(aimOf(WHERE)).toMatchObject({ target: { threadId: "t2" }, index: 2 });
});

test("a row acts from the mailbox it names, not the one the list shows", () => {
  const rows = new Map([[0, [row("t0", ["a"], ARCHIVE)]]]);
  expect(aimOf({ ...WHERE, rows, cursor: 0 })?.place.mailbox.id).toBe(ARCHIVE);
  const lost = new Map([[0, [row("t0", ["a"], "gone")]]]);
  expect(aimOf({ ...WHERE, rows: lost, cursor: 0 })).toBeNull();
});

test("an open thread no page holds aims through its detail from the mailbox it acts from", () => {
  expect(aimOf({ ...WHERE, openThreadId: "t9", detail: DETAIL })).toMatchObject({
    target: { threadId: "t9", memberIds: ["e1", "e3"] },
    index: null,
    place: { mailbox: { id: INBOX_ID } },
  });
  const sent = aimOf({ ...WHERE, openThreadId: "t9", detail: DETAIL, openMailboxId: SENT });
  expect(sent?.target.memberIds).toEqual(["e2"]);
  expect(aimOf({ ...WHERE, openThreadId: "t9", detail: DETAIL, openMailboxId: null })).toBeNull();
  expect(aimOf({ ...WHERE, openThreadId: "t9" })).toBeNull();
  expect(aimOf({ ...WHERE, cursor: 7 })).toBeNull();
});

test("after an action the next row opens, or the list returns with the cursor on it", () => {
  expect(afterOf(ROWS, 0, "next")).toEqual({ open: ROWS.get(0)?.[1], index: 1 });
  expect(afterOf(ROWS, 0, "list")).toEqual({ close: true, index: 1 });
});

test("at the end of the list the thread closes onto the row before, or onto nothing", () => {
  expect(afterOf(ROWS, 2, "next")).toEqual({ close: true, index: 1 });
  expect(afterOf(new Map([[0, [row("t0")]]]), 0, "next")).toEqual({ close: true, index: null });
  expect(afterOf(ROWS, null, "next")).toEqual({ close: true, index: null });
});

test("rows that leave together are passed over for the one that takes the place", () => {
  const five = new Map([[0, ["t0", "t1", "t2", "t3", "t4"].map((id) => row(id))]]);
  expect(survivorOf(five, 1, new Set([1, 2]))).toMatchObject({ index: 3, after: true });
  expect(survivorOf(five, 4, new Set([3, 4]))).toMatchObject({ index: 2, after: false });
  expect(survivorOf(five, 0, new Set([0, 1, 2, 3, 4]))).toBeNull();
  expect(afterOf(five, 1, "next", new Set([1, 2]))).toEqual({ open: five.get(0)?.[3], index: 3 });
  expect(afterOf(five, 4, "next", new Set([2, 3, 4]))).toEqual({ close: true, index: 1 });
});

test("several threads read as unread while any is and starred while every one is", () => {
  const unread = { ...row("t0"), unread: true };
  const starred = { ...row("t1"), flagged: true };
  expect(combinedOf([aim(unread), aim(starred, 1)])).toEqual({ unread: true, flagged: false });
  expect(combinedOf([])).toEqual({ unread: false, flagged: false });
});

test("a flip on several threads takes the ones not on its side; a move takes every one", () => {
  const unread = { ...row("t0"), unread: true };
  const starred = { ...row("t1"), flagged: true };
  const aims = [aim(unread), aim(starred, 1)];
  const read = takenBy("read", aims);
  expect(read?.action).toBe("read");
  expect(read?.taken.map(({ target }) => target.threadId)).toEqual(["t0"]);
  const star = takenBy("star", aims);
  expect(star?.action).toBe("star");
  expect(star?.taken.map(({ target }) => target.threadId)).toEqual(["t0"]);
  expect(takenBy("archive", aims)?.taken).toHaveLength(2);
  expect(takenBy("archive", [])).toBeNull();
});

test("threads from several mailboxes each act from their own: archive leaves the archived ones alone", () => {
  const aims = [aim(row("t0")), aim(row("t1", ["b"], ARCHIVE), 1)];
  const archive = takenBy("archive", aims);
  expect(archive?.action).toBe("archive");
  expect(archive?.taken.map(({ target }) => target.threadId)).toEqual(["t0"]);
});

test("delete and spam follow the threads outside Junk and Trash, and take the bins' own forms only when every thread stands there", () => {
  const inbox = aim(row("t0"));
  const trashed = aim(row("t1", ["b"], TRASH), 1);
  const junked = aim(row("t2", ["c"], JUNK), 2);
  const mixed = takenBy("delete", [trashed, inbox, junked]);
  expect(mixed?.action).toBe("delete");
  expect(mixed?.taken.map(({ target }) => target.threadId)).toEqual(["t0"]);
  const bins = takenBy("delete", [trashed, junked]);
  expect(bins?.action).toBe("deleteForever");
  expect(bins?.taken).toHaveLength(2);
  expect(takenBy("spam", [junked, inbox])?.taken.map(({ target }) => target.threadId)).toEqual([
    "t0",
  ]);
  expect(takenBy("spam", [junked])?.action).toBe("notSpam");
});

test("a thread whose mailbox offers no action of the kind is never taken", () => {
  const readOnly = placesOf(MAILBOXES);
  const kept = aimAt(row("t0"), 0, readOnly);
  expect(kept === null ? null : takenBy("archive", [kept])?.taken).toEqual([]);
});
