// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { accountRow, mockAccounts } from "./account-mocks";
import { FIXED_NOW } from "./mail-corpus";
import { MAILBOXES, mockMail } from "./mail-mocks";
import type { MailboxBody } from "./mail-mocks";
import {
  INTERACTION_BUDGET_MS,
  NO_SLOWDOWN,
  benchmarkIndexOf,
  onPhoneProfile,
} from "./phone-profile";
import type { HostSpeed } from "./phone-profile";
import { mockSignedIn } from "./session-mocks";
import { VIEWPORTS } from "./sweep";

// The frame budget at 60 Hz: the main thread's own work for one frame,
// from the scroll it answers to the paint, must fit in it.
const FRAME_BUDGET_MS = 16.7;
// The scroll: this many frames at a fast fling of this many rows each,
// the first few left out while the page warms up.
const SCROLL_FRAMES = 600;
const ROWS_PER_FRAME = 4;
const WARM_UP_FRAMES = 5;
// The mailbox: fifty thousand messages in threads.
const CORPUS_SIZE = 50_000;
const CORPUS_UNREAD = 300;
// The scroll and the trace need longer than a plain test.
const TRACE_TIMEOUT_MS = 180_000;
const PERCENTILE = 0.95;
// The trace holds the main thread's tasks and the marks the frames leave.
const TRACE_CATEGORIES = ["toplevel", "blink.user_timing"];
// The marks a frame leaves in the trace: one in its animation callback,
// one in the timer queued there, which runs once the frame has painted
// and the tasks queued before it are done.
const FRAME_MARK = "frame";
const FRAME_END_MARK = "frame-end";
// One task of a thread, with the thread's own CPU time in tdur.
const TASK_EVENT = "ThreadControllerImpl::RunTask";
// Trace times are in microseconds.
const MICROSECONDS_PER_MS = 1000;

const ROWS = [accountRow(FIXED_NOW)];

// The inbox at the corpus size; every other mailbox as the fixtures have it.
function sized(row: MailboxBody): MailboxBody {
  return {
    ...row,
    totalEmails: CORPUS_SIZE,
    totalThreads: CORPUS_SIZE,
    unreadEmails: CORPUS_UNREAD,
  };
}

const INBOX_50K: MailboxBody[] = MAILBOXES.map((row) => (row.role === "inbox" ? sized(row) : row));

// The clock stays the browser's own: a faked one would run the timers
// and the frames the trace counts on.
async function openInbox(page: Page): Promise<void> {
  await mockSignedIn(page);
  await mockMail(page, INBOX_50K);
  await mockAccounts(page, ROWS);
  await page.goto("/mail/acc-1/mb-inbox");
  await expect(page.getByRole("grid").locator('[aria-rowindex="1"]')).toBeVisible();
}

// Scrolls the grid by rows on every animation frame and marks each frame
// in the trace twice: in its animation callback and in the timer queued
// there. The run ends in the task after the last timer, so that timer's
// task is whole in the trace.
function scrollFrames(page: Page): Promise<void> {
  return page.evaluate(
    async ({ frames, rowsPerFrame, mark, endMark }) => {
      const grid = document.querySelector('[role="grid"]');
      const row = grid?.querySelector('[role="row"]');
      if (!(grid instanceof HTMLElement) || !(row instanceof HTMLElement)) {
        throw new Error("the grid has no rows");
      }
      const step = row.getBoundingClientRect().height * rowsPerFrame;
      await new Promise<void>((resolve) => {
        let scrolled = 0;
        let closed = 0;
        const tick = (): void => {
          performance.mark(mark);
          grid.scrollTop += step;
          scrolled += 1;
          setTimeout(() => {
            performance.mark(endMark);
            closed += 1;
            if (closed === frames) {
              setTimeout(resolve, 0);
            }
          }, 0);
          if (scrolled < frames) {
            requestAnimationFrame(tick);
          }
        };
        requestAnimationFrame(tick);
      });
    },
    {
      frames: SCROLL_FRAMES,
      rowsPerFrame: ROWS_PER_FRAME,
      mark: FRAME_MARK,
      endMark: FRAME_END_MARK,
    },
  );
}

// A trace event in the fields the frames are read from. Times are in
// microseconds; an instant has no length and a task too short to measure
// carries no CPU time.
interface TraceEvent {
  ph: string;
  cat: string;
  name: string;
  pid: number;
  tid: number;
  ts: number;
  dur?: number;
  tdur?: number;
}

function isTraceEvent(value: unknown): value is TraceEvent {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const strings = ["ph", "cat", "name"].every((key) => typeof Reflect.get(value, key) === "string");
  const numbers = ["pid", "tid", "ts"].every((key) => typeof Reflect.get(value, key) === "number");
  const spans = ["dur", "tdur"].every((key) =>
    ["undefined", "number"].includes(typeof Reflect.get(value, key)),
  );
  return strings && numbers && spans;
}

// The events of a trace as Chromium writes it.
function eventsOf(trace: Buffer): TraceEvent[] {
  const parsed: unknown = JSON.parse(trace.toString());
  const listed: unknown =
    typeof parsed === "object" && parsed !== null ? Reflect.get(parsed, "traceEvents") : null;
  return Array.isArray(listed) ? listed.filter((event) => isTraceEvent(event)) : [];
}

function isMark(event: TraceEvent, name: string): boolean {
  return event.ph === "I" && event.cat === "blink.user_timing" && event.name === name;
}

// The marks of one name, in the order they were set.
function marksOf(events: readonly TraceEvent[], name: string): TraceEvent[] {
  return events.filter((event) => isMark(event, name)).toSorted((one, other) => one.ts - other.ts);
}

// A task that ran to its end inside the trace.
function isWholeTask(event: TraceEvent): boolean {
  return event.ph === "X" && event.name === TASK_EVENT;
}

// The task a mark was left in.
function taskHolding(tasks: readonly TraceEvent[], mark: TraceEvent): TraceEvent {
  const task = tasks.find(
    (candidate) => candidate.ts <= mark.ts && mark.ts <= candidate.ts + (candidate.dur ?? 0),
  );
  if (task === undefined) {
    throw new Error("a frame mark lies outside every task");
  }
  return task;
}

// The CPU time of the tasks from one through another, in milliseconds.
function cpuThrough(tasks: readonly TraceEvent[], from: TraceEvent, through: TraceEvent): number {
  const inside = tasks.filter((task) => task.ts >= from.ts && task.ts <= through.ts);
  return inside.reduce((sum, task) => sum + (task.tdur ?? 0), 0) / MICROSECONDS_PER_MS;
}

// Each frame's CPU time: the thread time of the task holding its first
// mark, which carries the scroll event, the render, the layout and the
// paint, plus every task of the thread after it through the one holding
// its closing mark. The main thread is the one the marks landed on.
function frameCpuOf(events: readonly TraceEvent[]): number[] {
  const marks = marksOf(events, FRAME_MARK);
  const endMarks = marksOf(events, FRAME_END_MARK);
  const first = marks[0];
  if (first === undefined || endMarks.length !== marks.length) {
    throw new Error("the trace holds no frame or an unclosed one");
  }
  const tasks = events.filter(
    (event) => isWholeTask(event) && event.pid === first.pid && event.tid === first.tid,
  );
  return marks.flatMap((mark, frame) => {
    const endMark = endMarks.at(frame);
    return endMark === undefined
      ? []
      : [cpuThrough(tasks, taskHolding(tasks, mark), taskHolding(tasks, endMark))];
  });
}

interface Trace {
  // The long frames as "frame: ms", in order.
  long: string[];
  max: number;
  p95: number;
}

function traceOf(cpu: readonly number[]): Trace {
  const measured = cpu.slice(WARM_UP_FRAMES);
  const sorted = measured.toSorted((one, other) => one - other);
  return {
    long: measured.flatMap((work, frame) =>
      work > FRAME_BUDGET_MS ? [`${String(frame + WARM_UP_FRAMES)}: ${work.toFixed(1)}`] : [],
    ),
    max: sorted.at(-1) ?? 0,
    p95: sorted.at(Math.floor(sorted.length * PERCENTILE)) ?? 0,
  };
}

interface Profile extends HostSpeed {
  name: string;
}

// Traces the fling and holds every frame to the budget. The figures go
// to stdout first, so the log carries them whatever the outcome.
async function expectSmoothScroll(page: Page, profile: Profile): Promise<void> {
  const browser = page.context().browser();
  if (browser === null) {
    throw new Error("the page has no browser to trace");
  }
  await browser.startTracing(page, { categories: TRACE_CATEGORIES });
  await scrollFrames(page);
  const cpu = frameCpuOf(eventsOf(await browser.stopTracing()));
  const trace = traceOf(cpu);
  console.log(
    `${profile.name} frames: benchmark index ${profile.benchmarkIndex.toFixed(0)}, CPU slowed ${profile.slowdown.toFixed(1)}x, max ${trace.max.toFixed(1)} ms, p95 ${trace.p95.toFixed(1)} ms, long at ${trace.long.join(", ") || "none"}`,
  );
  expect(cpu, `${profile.name}: frames in the trace`).toHaveLength(SCROLL_FRAMES);
  expect(trace.long, `${profile.name}: frames past ${String(FRAME_BUDGET_MS)} ms`).toEqual([]);
}

test("the fifty-thousand-row list scrolls without a long frame on the desktop profile", async ({
  page,
}) => {
  test.setTimeout(TRACE_TIMEOUT_MS);
  await page.setViewportSize(VIEWPORTS[1]);
  const benchmarkIndex = await benchmarkIndexOf(page);
  await openInbox(page);
  await expectSmoothScroll(page, { name: "desktop", benchmarkIndex, slowdown: NO_SLOWDOWN });
});

test("the fifty-thousand-row list scrolls without a long frame on the phone profile", async ({
  page,
}) => {
  test.setTimeout(TRACE_TIMEOUT_MS);
  await page.setViewportSize(VIEWPORTS[0]);
  await onPhoneProfile(page, async (speed) => {
    await openInbox(page);
    await expectSmoothScroll(page, { name: "phone", ...speed });
  });
});

// Where the page keeps the measurement between the two evaluations.
const MEASUREMENT = "__keyToPaint";

// The time from the key going down to the first frame painted with the
// focus on another element, measured inside the page: a frame that
// finds the focus moved is the one that paints it. The listeners go in
// before the key, in an evaluation of their own, since a key sent
// beside one can land first.
async function keyToPaint(page: Page, key: string): Promise<number> {
  await page.evaluate((slot) => {
    const measured = new Promise<number>((resolve) => {
      const before = document.activeElement;
      let pressed = 0;
      document.addEventListener(
        "keydown",
        () => {
          pressed = performance.now();
        },
        { capture: true, once: true },
      );
      const frame = (now: number): void => {
        if (pressed > 0 && document.activeElement !== before) {
          resolve(now - pressed);
        } else {
          requestAnimationFrame(frame);
        }
      };
      requestAnimationFrame(frame);
    });
    Reflect.set(window, slot, measured);
  }, MEASUREMENT);
  await page.keyboard.press(key);
  const elapsed = await page.evaluate((slot): unknown => Reflect.get(window, slot), MEASUREMENT);
  if (typeof elapsed !== "number") {
    throw new Error("the page measured nothing");
  }
  return elapsed;
}

// The keys of the list, then Enter, which opens the row and puts the
// focus on the thread's title, then Escape, which brings it back; after
// those the palette's key and the overlay's, each with the Escape that
// closes it.
const MEASURED_KEYS = [
  "j",
  "k",
  "ArrowDown",
  "ArrowUp",
  "End",
  "Home",
  "Enter",
  "Escape",
  "ControlOrMeta+k",
  "Escape",
  "?",
  "Escape",
];

test("a key moves the focus within the interaction budget on the long list", async ({ page }) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await openInbox(page);
  await page.getByRole("grid").locator('[aria-rowindex="1"]').focus();
  for (const key of MEASURED_KEYS) {
    const elapsed = await keyToPaint(page, key);
    test
      .info()
      .annotations.push({ type: `${key} to paint`, description: `${elapsed.toFixed(1)} ms` });
    expect(elapsed, key).toBeLessThan(INTERACTION_BUDGET_MS);
  }
});
