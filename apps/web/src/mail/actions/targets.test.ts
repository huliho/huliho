// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow, ThreadDetail } from "@huliho/core";
import { expect, test } from "vitest";

import { afterOf, aimOf, targetOfDetail } from "./targets";

function row(threadId: string, memberIds = [`${threadId}-1`]): ListRow {
  return {
    id: memberIds[0] ?? threadId,
    threadId,
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

// The first page with t0, t1 and t2.
const ROWS = new Map([[0, [row("t0"), row("t1"), row("t2")]]]);

// The thread t9: e1 read in the inbox, e2 flagged in Sent, e3 unread in
// the inbox.
const DETAIL: ThreadDetail = {
  thread: {
    id: "t9",
    emailIds: ["e1", "e2", "e3"],
    members: {
      e1: { keywords: { $seen: true }, mailboxIds: { inbox: true } },
      e2: { keywords: { $seen: true, $flagged: true }, mailboxIds: { sent: true } },
      e3: { keywords: {}, mailboxIds: { inbox: true } },
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
  mailboxId: "inbox",
};

test("the open thread takes its members in the mailbox shown, in the thread's order", () => {
  expect(targetOfDetail(DETAIL, "inbox")).toEqual({
    threadId: "t9",
    memberIds: ["e1", "e3"],
    unread: true,
    flagged: false,
  });
  expect(targetOfDetail(DETAIL, "sent")).toEqual({
    threadId: "t9",
    memberIds: ["e2"],
    unread: false,
    flagged: true,
  });
  expect(targetOfDetail(DETAIL, "archive")).toBeNull();
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

test("an open thread no page holds aims through its detail, without a place in the list", () => {
  expect(aimOf({ ...WHERE, openThreadId: "t9", detail: DETAIL })).toMatchObject({
    target: { threadId: "t9", memberIds: ["e1", "e3"] },
    index: null,
  });
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
