// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";
import { pageFunctions } from "lighthouse/core/lib/page-functions.js";

import { accountRow, mockAccounts } from "./account-mocks";
import { FIXED_NOW } from "./mail-corpus";
import { MAILBOXES, mockMail } from "./mail-mocks";
import type { MailboxBody } from "./mail-mocks";
import { mockSignedIn } from "./session-mocks";
import { VIEWPORTS } from "./sweep";

// The frame budget at 60 Hz: the main thread's work for one frame, from
// the scroll it answers to the paint, must fit in it.
const FRAME_BUDGET_MS = 16.7;
// From a keypress to the painted focus.
const INTERACTION_BUDGET_MS = 100;
// The middle of the high-end mobile bracket, 800 to 1200, in Lighthouse's
// docs/throttling.md. The phone profile slows the CPU by the host's own
// index, measured before throttling, over this one, so the profile means
// the same on every host.
const MID_RANGE_PHONE_BENCHMARK_INDEX = 1000;
// No slowdown: the floor, for a host no faster than the phone.
const NO_SLOWDOWN = 1;
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

// Scrolls the grid by rows on every animation frame and answers how
// long the main thread was busy for each frame: from the scroll event
// that opens it, through the render and the layout, to the paint, which
// the task after the frame marks the end of.
function scrollFrames(page: Page): Promise<number[]> {
  return page.evaluate(
    async ({ frames, rowsPerFrame }) => {
      const grid = document.querySelector('[role="grid"]');
      const row = grid?.querySelector('[role="row"]');
      if (!(grid instanceof HTMLElement) || !(row instanceof HTMLElement)) {
        throw new Error("the grid has no rows");
      }
      const step = row.getBoundingClientRect().height * rowsPerFrame;
      const busy: number[] = [];
      let opened: number | null = null;
      document.addEventListener(
        "scroll",
        () => {
          opened ??= performance.now();
        },
        { capture: true },
      );
      await new Promise<void>((resolve) => {
        let ticks = 0;
        const tick = (now: number): void => {
          const start = opened ?? now;
          opened = null;
          grid.scrollTop += step;
          ticks += 1;
          setTimeout(() => {
            busy.push(performance.now() - start);
            if (busy.length === frames) {
              resolve();
            }
          }, 0);
          if (ticks < frames) {
            requestAnimationFrame(tick);
          }
        };
        requestAnimationFrame(tick);
      });
      return busy;
    },
    { frames: SCROLL_FRAMES, rowsPerFrame: ROWS_PER_FRAME },
  );
}

interface Trace {
  // The long frames as "frame: ms", in order.
  long: string[];
  max: number;
  p95: number;
}

function traceOf(busy: readonly number[]): Trace {
  const measured = busy.slice(WARM_UP_FRAMES);
  const sorted = measured.toSorted((one, other) => one - other);
  return {
    long: measured.flatMap((work, frame) =>
      work > FRAME_BUDGET_MS ? [`${String(frame + WARM_UP_FRAMES)}: ${work.toFixed(1)}`] : [],
    ),
    max: sorted.at(-1) ?? 0,
    p95: sorted.at(Math.floor(sorted.length * PERCENTILE)) ?? 0,
  };
}

async function expectSmoothScroll(page: Page, profile: string): Promise<void> {
  const trace = traceOf(await scrollFrames(page));
  test.info().annotations.push({
    type: `${profile} frames`,
    description: `max ${trace.max.toFixed(1)} ms, p95 ${trace.p95.toFixed(1)} ms, long at ${trace.long.join(", ") || "none"}`,
  });
  expect(trace.long, `${profile}: frames past ${String(FRAME_BUDGET_MS)} ms`).toEqual([]);
}

test("the fifty-thousand-row list scrolls without a long frame on the desktop profile", async ({
  page,
}) => {
  test.setTimeout(TRACE_TIMEOUT_MS);
  await page.setViewportSize(VIEWPORTS[1]);
  await openInbox(page);
  await expectSmoothScroll(page, "desktop");
});

function benchmarkIndexOf(page: Page): Promise<number> {
  return page.evaluate(pageFunctions.computeBenchmarkIndex);
}

test("the fifty-thousand-row list scrolls without a long frame on the phone profile", async ({
  page,
}) => {
  test.setTimeout(TRACE_TIMEOUT_MS);
  await page.setViewportSize(VIEWPORTS[0]);
  const benchmarkIndex = await benchmarkIndexOf(page);
  const slowdown = Math.max(NO_SLOWDOWN, benchmarkIndex / MID_RANGE_PHONE_BENCHMARK_INDEX);
  test.info().annotations.push({
    type: "phone profile",
    description: `benchmark index ${benchmarkIndex.toFixed(0)}, CPU slowed ${slowdown.toFixed(1)}x`,
  });
  const session = await page.context().newCDPSession(page);
  await session.send("Emulation.setCPUThrottlingRate", { rate: slowdown });
  await openInbox(page);
  await expectSmoothScroll(page, "phone");
  await session.send("Emulation.setCPUThrottlingRate", { rate: NO_SLOWDOWN });
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
