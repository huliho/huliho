// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Locator, Page } from "@playwright/test";
import { expect } from "@playwright/test";

import { accountRow, mockAccounts } from "./account-mocks";
import { FIXED_NOW } from "./mail-corpus";
import { MAILBOXES, mockMail } from "./mail-mocks";
import type { MailboxBody } from "./mail-mocks";
import { mockSignedIn } from "./session-mocks";

// The fling over the long list: this many frames of this many rows each,
// the first few left out while the page warms up.
export const SCROLL_FRAMES = 600;
export const ROWS_PER_FRAME = 4;
export const WARM_UP_FRAMES = 5;
// The marks a frame leaves in the trace: one in its animation callback,
// one in the timer queued there, which runs once the frame has painted
// and the tasks queued before it are done.
export const FRAME_MARK = "frame";
export const FRAME_END_MARK = "frame-end";
// An inbox of fifty thousand threads.
const INBOX_THREADS = 50_000;
const INBOX_UNREAD = 300;
// One task of a thread.
const TASK_EVENT = "ThreadControllerImpl::RunTask";

const ACCOUNTS = [accountRow(FIXED_NOW)];

function atFiftyThousand(row: MailboxBody): MailboxBody {
  return {
    ...row,
    totalEmails: INBOX_THREADS,
    totalThreads: INBOX_THREADS,
    unreadEmails: INBOX_UNREAD,
  };
}

const MAILBOXES_50K = MAILBOXES.map((row) => (row.role === "inbox" ? atFiftyThousand(row) : row));

export function firstRow(page: Page): Locator {
  return page.getByRole("grid").locator('[aria-rowindex="1"]');
}

// The clock stays the browser's own: a faked one would run the timers
// and the frames the traces count on.
export async function openLongInbox(page: Page): Promise<void> {
  await mockSignedIn(page);
  await mockMail(page, MAILBOXES_50K);
  await mockAccounts(page, ACCOUNTS);
  await page.goto("/mail/acc-1/mb-inbox");
  await expect(firstRow(page), "the inbox's first row").toBeVisible();
}

export function isObject(value: unknown): value is object {
  return typeof value === "object" && value !== null;
}

// A trace event in the fields the frames are read from. Times are in
// microseconds; an instant has no length and a task too short to measure
// carries no CPU time.
export interface TraceEvent {
  ph: string;
  cat: string;
  name: string;
  pid: number;
  tid: number;
  ts: number;
  dur?: number;
  tdur?: number;
  args?: unknown;
}

function isTraceEvent(value: unknown): value is TraceEvent {
  if (!isObject(value)) {
    return false;
  }
  const typed = (keys: string[], type: string): boolean =>
    keys.every((key) => typeof Reflect.get(value, key) === type);
  const spans = ["dur", "tdur"].every((key) =>
    ["undefined", "number"].includes(typeof Reflect.get(value, key)),
  );
  return typed(["ph", "cat", "name"], "string") && typed(["pid", "tid", "ts"], "number") && spans;
}

// The events of a trace as Chromium writes it.
export function eventsOf(trace: Buffer): TraceEvent[] {
  const parsed: unknown = JSON.parse(trace.toString());
  const listed: unknown = isObject(parsed) ? Reflect.get(parsed, "traceEvents") : null;
  return Array.isArray(listed) ? listed.filter((event) => isTraceEvent(event)) : [];
}

export function endOf(event: TraceEvent): number {
  return event.ts + (event.dur ?? 0);
}

// The marks of one name, in the order they were set.
function marksOf(events: readonly TraceEvent[], name: string): TraceEvent[] {
  return events
    .filter((event) => event.ph === "I" && event.cat === "blink.user_timing" && event.name === name)
    .toSorted((one, other) => one.ts - other.ts);
}

// The task a mark was left in.
function taskHolding(tasks: readonly TraceEvent[], mark: TraceEvent): TraceEvent {
  const task = tasks.find((candidate) => candidate.ts <= mark.ts && mark.ts <= endOf(candidate));
  if (task === undefined) {
    throw new Error("a frame mark lies outside every task");
  }
  return task;
}

// The task holding a frame's first mark and the task holding its closing
// mark.
interface FrameTasks {
  from: TraceEvent;
  through: TraceEvent;
}

// The main thread, the one the marks landed on: its complete events, its
// tasks and each frame's first and closing task.
interface FrameTrace {
  thread: TraceEvent[];
  tasks: TraceEvent[];
  frames: FrameTasks[];
}

export function frameTraceOf(events: readonly TraceEvent[]): FrameTrace {
  const marks = marksOf(events, FRAME_MARK);
  const endMarks = marksOf(events, FRAME_END_MARK);
  const first = marks[0];
  if (first === undefined || endMarks.length !== marks.length) {
    throw new Error("the trace holds no frame or an unclosed one");
  }
  const thread = events.filter(
    (event) => event.ph === "X" && event.pid === first.pid && event.tid === first.tid,
  );
  const tasks = thread.filter((event) => event.name === TASK_EVENT);
  const frames = marks.flatMap((mark, frame) => {
    const endMark = endMarks.at(frame);
    return endMark === undefined
      ? []
      : [{ from: taskHolding(tasks, mark), through: taskHolding(tasks, endMark) }];
  });
  return { thread, tasks, frames };
}
