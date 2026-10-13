// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import {
  FRAME_END_MARK,
  FRAME_MARK,
  ROWS_PER_FRAME,
  SCROLL_FRAMES,
  WARM_UP_FRAMES,
  eventsOf,
  frameTraceOf,
  openLongInbox,
} from "../long-list";
import type { TraceEvent } from "../long-list";
import { NO_SLOWDOWN, benchmarkIndexOf, onPhoneProfile } from "../phone-profile";
import type { HostSpeed } from "../phone-profile";
import { VIEWPORTS } from "../sweep";

// The frame budget at 60 Hz: the main thread's own work for one frame,
// from the scroll it answers to the paint, must fit in it.
const FRAME_BUDGET_MS = 16.7;
// The scroll and the trace need longer than a plain test.
const TRACE_TIMEOUT_MS = 180_000;
const PERCENTILE = 0.95;
// The trace holds the main thread's tasks and the marks the frames leave.
const TRACE_CATEGORIES = ["toplevel", "blink.user_timing"];
// Trace times are in microseconds.
const MICROSECONDS_PER_MS = 1000;

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

// The CPU time of the tasks from one through another, in milliseconds.
function cpuThrough(tasks: readonly TraceEvent[], from: TraceEvent, through: TraceEvent): number {
  const inside = tasks.filter((task) => task.ts >= from.ts && task.ts <= through.ts);
  return inside.reduce((sum, task) => sum + (task.tdur ?? 0), 0) / MICROSECONDS_PER_MS;
}

// Each frame's CPU time: the thread time of the task holding its first
// mark, which carries the scroll event, the render, the layout and the
// paint, plus every task of the thread after it through the one holding
// its closing mark.
function frameCpuOf(events: readonly TraceEvent[]): number[] {
  const { tasks, frames } = frameTraceOf(events);
  return frames.map(({ from, through }) => cpuThrough(tasks, from, through));
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
  await openLongInbox(page);
  await expectSmoothScroll(page, { name: "desktop", benchmarkIndex, slowdown: NO_SLOWDOWN });
});

test("the fifty-thousand-row list scrolls without a long frame on the phone profile", async ({
  page,
}) => {
  test.setTimeout(TRACE_TIMEOUT_MS);
  await page.setViewportSize(VIEWPORTS[0]);
  await onPhoneProfile(page, async (speed) => {
    await openLongInbox(page);
    await expectSmoothScroll(page, { name: "phone", ...speed });
  });
});
