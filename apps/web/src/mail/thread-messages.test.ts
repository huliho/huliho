// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { THREAD, threadDetail } from "./fixtures";
import { messagesOf, planMessages, subjectOf } from "./thread-messages";

const COUNT = 14;

test("the messages come oldest first and a header the detail lacks is left out", () => {
  const messages = messagesOf(THREAD);
  expect(messages.map((email) => email.id)).toEqual(THREAD.thread.emailIds);
  expect(messages.at(-1)?.id).toBe("e-3");
  const { "e-3-m2": _first, ...rest } = THREAD.emails;
  expect(messagesOf({ ...THREAD, emails: rest })).toHaveLength(COUNT - 1);
});

test("with every message read the newest opens, two stay in sight and the rest wait", () => {
  const plan = planMessages(messagesOf(THREAD));
  expect(plan.olderCount).toBe(COUNT - 3);
  expect(plan.messages.map((message) => message.expanded)).toEqual([
    ...Array.from({ length: COUNT - 1 }, () => false),
    true,
  ]);
  expect(plan.messages.filter((message) => !message.older)).toHaveLength(3);
  expect(plan.messages.every((message) => !message.unread)).toBe(true);
});

test("an unread message opens wherever it stands and the two before it stay in sight", () => {
  const plan = planMessages(messagesOf(threadDetail([4])));
  expect(plan.olderCount).toBe(2);
  expect(plan.messages[4]).toMatchObject({ unread: true, expanded: true, older: false });
  expect(plan.messages[2]).toMatchObject({ expanded: false, older: false });
  expect(plan.messages[1]).toMatchObject({ older: true });
  expect(plan.messages.at(-1)).toMatchObject({ expanded: true });
});

test("a thread of one or two messages hides nothing", () => {
  const messages = messagesOf(THREAD).slice(-2);
  const plan = planMessages(messages);
  expect(plan.olderCount).toBe(0);
  expect(plan.messages.map((message) => message.expanded)).toEqual([false, true]);
  expect(planMessages([])).toEqual({ messages: [], olderCount: 0 });
});

test("the subject is the first message's, null when it has none", () => {
  const messages = messagesOf(THREAD);
  expect(subjectOf(messages)).toBe("Offerte badkamerrenovatie, herziene versie");
  const first = messages[0];
  expect(first === undefined ? null : subjectOf([{ ...first, subject: " " }])).toBeNull();
  expect(subjectOf([])).toBeNull();
});
