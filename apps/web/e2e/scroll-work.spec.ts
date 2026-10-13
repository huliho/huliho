// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page, Request } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { accountRow, mockAccounts } from "./account-mocks";
import { FIXED_NOW } from "./mail-corpus";
import { MAILBOXES, mockMail } from "./mail-mocks";
import type { MailboxBody } from "./mail-mocks";
import { mockSignedIn } from "./session-mocks";
import { VIEWPORTS } from "./sweep";

// The fling: this many frames of this many rows each, the first few left
// out while the page warms up.
const SCROLL_FRAMES = 600;
const ROWS_PER_FRAME = 4;
const WARM_UP_FRAMES = 5;
// An inbox of fifty thousand threads.
const INBOX_THREADS = 50_000;
const INBOX_UNREAD = 300;
// The list renders this many rows past the ones in view on either side
// and keeps the cursor's row mounted when a scroll leaves it behind.
const LIST_OVERSCAN = 3;
const CURSOR_ROWS = 1;
// One page of the list holds this many rows; the list asks for the page
// this many rows past the ones in view.
const PAGE_ROWS = 100;
const PREFETCH_ROWS = 40;
// What one page costs: the query, the headers, the threads and the
// states of their members.
const CALLS_PER_PAGE = { "Email/query": 1, "Email/get": 2, "Thread/get": 1 };
// The steps that lay the tree out. Every frame runs each once under the
// harness's scrollTop write in its animation callback. A frame where a
// page landed since the one before may run each once more: under the
// list's scrollTop read in the scroll event, or under no script at the
// start of the frame's update, ahead of a hit test.
const TREE_STEPS = ["Layout", "UpdateLayoutTree"];
const HARNESS_CHAIN = "FireAnimationFrame>FunctionCall";
const LANDING_CHAINS = new Set(["EventDispatch(scroll)>FunctionCall", ""]);
const STEP_CHAINS = new Set([HARNESS_CHAIN, ...LANDING_CHAINS]);
const ONCE = new Set([1]);
const AT_MOST_ONCE = new Set([0, 1]);
// The trace events that run script, the links of a step's chain.
const SCRIPT_EVENTS = new Set([
  "FireAnimationFrame",
  "EventDispatch",
  "TimerFire",
  "FunctionCall",
  "RunMicrotasks",
  "EvaluateScript",
]);
const TRACE_CATEGORIES = ["toplevel", "blink.user_timing", "devtools.timeline"];
const FRAME_MARK = "frame";
const FRAME_END_MARK = "frame-end";
const TASK_EVENT = "ThreadControllerImpl::RunTask";
// The fling and its trace need longer than a plain test.
const FLING_TIMEOUT_MS = 180_000;
// Where the page sends its JMAP calls.
const JMAP_API_PATH = /^\/api\/jmap\/[^/]+$/;

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

function firstRow(page: Page) {
  return page.getByRole("grid").locator('[aria-rowindex="1"]');
}

async function openInbox(page: Page): Promise<void> {
  await mockSignedIn(page);
  await mockMail(page, MAILBOXES_50K);
  await mockAccounts(page, ACCOUNTS);
  await page.goto("/mail/acc-1/mb-inbox");
  await expect(firstRow(page), "the inbox's first row").toBeVisible();
}

function isObject(value: unknown): value is object {
  return typeof value === "object" && value !== null;
}

// The JMAP methods a request for a page of the list calls, by name; none
// for any other request. A poll asks gets beside its changes but never a
// query, so only a request that queries counts.
function methodsOf(request: Request): string[] {
  if (request.method() !== "POST" || !JMAP_API_PATH.test(new URL(request.url()).pathname)) {
    return [];
  }
  const body: unknown = request.postDataJSON();
  const calls: unknown = isObject(body) ? Reflect.get(body, "methodCalls") : null;
  const names = Array.isArray(calls)
    ? calls.flatMap((call: unknown) => {
        const name: unknown = Array.isArray(call) ? call.at(0) : null;
        return typeof name === "string" ? [name] : [];
      })
    : [];
  return names.includes("Email/query") ? names : [];
}

interface FlingArgs {
  frames: number;
  rowsPerFrame: number;
  mark: string;
  endMark: string;
}

// The rows that fit the grid whole (its height over the row height,
// rounded down) and what the DOM held at each frame's closing mark: the
// rows mounted and the row elements added and removed since the one before.
interface FrameRows {
  fit: number;
  mounted: number[];
  added: number[];
  removed: number[];
}

// Runs in the page. Scrolls the grid by rows on every animation frame and
// marks each frame in the trace twice: in its animation callback and in
// the timer queued there, which counts the rows. The run ends in the task
// after the last timer, so that timer's task is whole in the trace.
async function flingInPage({ frames, rowsPerFrame, mark, endMark }: FlingArgs): Promise<FrameRows> {
  const grid = document.querySelector('[role="grid"]');
  const sizer = grid?.firstElementChild;
  const row = sizer?.firstElementChild;
  if (!grid || !sizer || !row) {
    throw new Error("the grid has no rows");
  }
  const height = row.getBoundingClientRect().height;
  const fit = Math.floor(grid.clientHeight / height);
  const rows: FrameRows = { fit, mounted: [], added: [], removed: [] };
  let added = 0;
  let removed = 0;
  const tally = (records: MutationRecord[]): void => {
    for (const record of records) {
      added += record.addedNodes.length;
      removed += record.removedNodes.length;
    }
  };
  const observer = new MutationObserver(tally);
  observer.observe(sizer, { childList: true });
  await new Promise<void>((resolve) => {
    let scrolled = 0;
    const close = (): void => {
      performance.mark(endMark);
      tally(observer.takeRecords());
      rows.mounted.push(sizer.childElementCount);
      rows.added.push(added);
      rows.removed.push(removed);
      added = 0;
      removed = 0;
      if (rows.mounted.length === frames) {
        setTimeout(resolve, 0);
      }
    };
    const tick = (): void => {
      performance.mark(mark);
      grid.scrollTop += height * rowsPerFrame;
      scrolled += 1;
      setTimeout(close, 0);
      if (scrolled < frames) {
        requestAnimationFrame(tick);
      }
    };
    requestAnimationFrame(tick);
  });
  observer.disconnect();
  return rows;
}

// A trace event in the fields the frames are read from, times in
// microseconds.
interface TraceEvent {
  ph: string;
  cat: string;
  name: string;
  pid: number;
  tid: number;
  ts: number;
  dur?: number;
  args?: unknown;
}

function isTraceEvent(value: unknown): value is TraceEvent {
  if (!isObject(value)) {
    return false;
  }
  const typed = (keys: string[], type: string): boolean =>
    keys.every((key) => typeof Reflect.get(value, key) === type);
  const dur: unknown = Reflect.get(value, "dur");
  return (
    typed(["ph", "cat", "name"], "string") &&
    typed(["pid", "tid", "ts"], "number") &&
    (dur === undefined || typeof dur === "number")
  );
}

function eventsOf(trace: Buffer): TraceEvent[] {
  const parsed: unknown = JSON.parse(trace.toString());
  const listed: unknown = isObject(parsed) ? Reflect.get(parsed, "traceEvents") : null;
  return Array.isArray(listed) ? listed.filter((event) => isTraceEvent(event)) : [];
}

function endOf(event: TraceEvent): number {
  return event.ts + (event.dur ?? 0);
}

// An event's name; a dispatch carries the DOM event it dispatched.
function labelOf(event: TraceEvent): string {
  const data: unknown = isObject(event.args) ? Reflect.get(event.args, "data") : null;
  const type: unknown = isObject(data) ? Reflect.get(data, "type") : null;
  return event.name === "EventDispatch" && typeof type === "string"
    ? `${event.name}(${type})`
    : event.name;
}

function marksNamed(events: readonly TraceEvent[], name: string): TraceEvent[] {
  return events
    .filter((event) => event.ph === "I" && event.cat === "blink.user_timing" && event.name === name)
    .toSorted((one, other) => one.ts - other.ts);
}

function taskAt(tasks: readonly TraceEvent[], mark: TraceEvent): TraceEvent {
  const task = tasks.find((candidate) => candidate.ts <= mark.ts && mark.ts <= endOf(candidate));
  if (task === undefined) {
    throw new Error("a frame mark lies outside every task");
  }
  return task;
}

// The scripts an event ran under, outermost first.
function chainOf(scripts: readonly TraceEvent[], event: TraceEvent): string {
  return scripts
    .filter((script) => script.ts <= event.ts && endOf(event) <= endOf(script))
    .toSorted((one, other) => one.ts - other.ts || endOf(other) - endOf(one))
    .map(labelOf)
    .join(">");
}

// One step that laid the tree out and the scripts it ran under.
interface StepRun {
  step: string;
  chain: string;
}

// Each frame's steps on the main thread, the thread the marks landed on:
// from the task holding the frame's first mark through the task holding
// its closing mark.
function frameStepsOf(events: readonly TraceEvent[]): StepRun[][] {
  const opened = marksNamed(events, FRAME_MARK);
  const closed = marksNamed(events, FRAME_END_MARK);
  const main = opened[0];
  if (main === undefined || closed.length !== opened.length) {
    throw new Error("the trace holds no frame or an unclosed one");
  }
  const thread = events.filter(
    (event) => event.ph === "X" && event.pid === main.pid && event.tid === main.tid,
  );
  const tasks = thread.filter((event) => event.name === TASK_EVENT);
  const relevant = thread.filter(
    (event) => TREE_STEPS.includes(event.name) || SCRIPT_EVENTS.has(event.name),
  );
  return opened.flatMap((mark, frame) => {
    const close = closed.at(frame);
    if (close === undefined) {
      return [];
    }
    const from = taskAt(tasks, mark).ts;
    const through = endOf(taskAt(tasks, close));
    const inside = relevant.filter((event) => event.ts >= from && endOf(event) <= through);
    const scripts = inside.filter((event) => SCRIPT_EVENTS.has(event.name));
    const steps = inside.filter((event) => TREE_STEPS.includes(event.name));
    return [steps.map((event) => ({ step: event.name, chain: chainOf(scripts, event) }))];
  });
}

interface Fling {
  name: string;
  rows: FrameRows;
  steps: StepRun[][];
  calls: ReadonlyMap<string, number>;
}

// Flings the list under a trace, counting the JMAP calls its requests make.
async function flingCounted(page: Page, name: string): Promise<Fling> {
  const browser = page.context().browser();
  if (browser === null) {
    throw new Error("the page has no browser to trace");
  }
  const calls = new Map(Object.keys(CALLS_PER_PAGE).map((method) => [method, 0]));
  const count = (request: Request): void => {
    for (const method of methodsOf(request)) {
      const seen = calls.get(method);
      if (seen !== undefined) {
        calls.set(method, seen + 1);
      }
    }
  };
  page.context().on("request", count);
  await browser.startTracing(page, { categories: TRACE_CATEGORIES });
  const rows = await page.evaluate(flingInPage, {
    frames: SCROLL_FRAMES,
    rowsPerFrame: ROWS_PER_FRAME,
    mark: FRAME_MARK,
    endMark: FRAME_END_MARK,
  });
  const steps = frameStepsOf(eventsOf(await browser.stopTracing()));
  page.context().off("request", count);
  return { name, rows, steps, calls };
}

// The distinct counts of the frames after the warm-up.
function steady(values: readonly number[]): number[] {
  return [...new Set(values.slice(WARM_UP_FRAMES))];
}

// How many frames hold each count that is not one of the allowed ones.
function framesOutside(
  counts: readonly number[],
  allowed: ReadonlySet<number>,
): Record<string, number> {
  const outside = new Map<number, number>();
  for (const count of counts.filter((held) => !allowed.has(held))) {
    outside.set(count, (outside.get(count) ?? 0) + 1);
  }
  return Object.fromEntries(outside);
}

function expectCountedWork({ name, rows, steps, calls }: Fling): void {
  // The rows in view are the ones that fit plus the one the grid's edge
  // cuts, so the last of them stands at the index `fit`.
  const mounted = rows.fit + 1 + LIST_OVERSCAN + LIST_OVERSCAN + CURSOR_ROWS;
  const reach = (scrolled: number): number =>
    Math.floor((scrolled + rows.fit + PREFETCH_ROWS) / PAGE_ROWS);
  const pages = reach(SCROLL_FRAMES * ROWS_PER_FRAME) - reach(0);
  expect(steps, `${name}: frames in the trace`).toHaveLength(SCROLL_FRAMES);
  expect(steady(rows.mounted), `${name}: rows mounted at a frame's close`).toEqual([mounted]);
  expect(steady(rows.added), `${name}: row elements added per frame`).toEqual([ROWS_PER_FRAME]);
  expect(steady(rows.removed), `${name}: row elements removed per frame`).toEqual([ROWS_PER_FRAME]);
  const measured = steps.slice(WARM_UP_FRAMES);
  const chains = new Set(measured.flat().map((run) => run.chain));
  const forced = [...chains].filter((chain) => !STEP_CHAINS.has(chain));
  expect(forced, `${name}: scripts a step ran under`).toEqual([]);
  for (const step of TREE_STEPS) {
    const runs = measured.map((frame) => frame.filter((run) => run.step === step));
    const harness = runs.map((frame) => frame.filter((run) => run.chain === HARNESS_CHAIN).length);
    const extra = runs.map((frame) => frame.filter((run) => LANDING_CHAINS.has(run.chain)).length);
    const once = `${name}: frames by how often the harness's write ran ${step}, if not once`;
    expect(framesOutside(harness, ONCE), once).toEqual({});
    const twice = `${name}: frames by how many further ${step} they ran, if more than one`;
    expect(framesOutside(extra, AT_MOST_ONCE), twice).toEqual({});
    const further = extra.reduce((sum, count) => sum + count, 0);
    const most = `${name}: frames with a further ${step}, at most ${String(pages)}`;
    expect(further, most).toBeLessThanOrEqual(pages);
  }
  const perPage = Object.entries(CALLS_PER_PAGE).map(([method, each]) => [method, each * pages]);
  const asked = `${name}: JMAP calls for ${String(pages)} pages`;
  expect(Object.fromEntries(calls), asked).toEqual(Object.fromEntries(perPage));
}

for (const viewport of VIEWPORTS) {
  test(`the fling's work per frame on the ${viewport.name} viewport`, async ({ page }) => {
    test.setTimeout(FLING_TIMEOUT_MS);
    await page.setViewportSize(viewport);
    await openInbox(page);
    expectCountedWork(await flingCounted(page, viewport.name));
  });
}

// What answers a key: the focus on another element (never the body), a
// dialog in the DOM or the focus back on the element the dialog opened from.
type Answer = "moved" | "dialog" | "returned";

// The list's keys from its first row, then the palette's key and the
// overlay's, each followed by the Escape that closes its dialog.
const LIST_KEYS = ["j", "k", "ArrowDown", "ArrowUp", "End", "Home"];
const DIALOG_KEYS = ["ControlOrMeta+k", "?"];
const KEY_ANSWERS: readonly [string, Answer][] = [
  ...LIST_KEYS.map((key): [string, Answer] => [key, "moved"]),
  ...DIALOG_KEYS.flatMap((key): [string, Answer][] => [
    [key, "dialog"],
    ["Escape", "returned"],
  ]),
];
// The page stops counting at this many frames, far past any answer, so a
// key that never answers reads as a count.
const FRAMES_CAP = 120;
// Where the page keeps the count and the element a dialog opened from.
const COUNT_SLOT = "__framesToAnswer";
const ORIGIN_SLOT = "__dialogOrigin";

interface Listening {
  key: string;
  answer: Answer;
  cap: number;
  slot: string;
  origin: string;
}

// Runs in the page. Counts the animation frames that run after the key
// goes down and before its answer shows: none when the key's own task
// answered it.
function countFramesToAnswer({ key, answer, cap, slot, origin }: Listening): void {
  const before = document.activeElement;
  if (answer === "dialog") {
    Reflect.set(window, origin, before);
  }
  const answered = (): boolean => {
    if (answer === "moved") {
      const active = document.activeElement;
      return active !== null && active !== document.body && active !== before;
    }
    if (answer === "dialog") {
      return document.querySelector('[role="dialog"]') !== null;
    }
    return document.activeElement === Reflect.get(window, origin);
  };
  let pressed = false;
  const onKey = (event: KeyboardEvent): void => {
    pressed ||= event.key === key;
  };
  document.addEventListener("keydown", onKey, { capture: true });
  const counted = new Promise<number>((resolve) => {
    let frames = 0;
    const frame = (): void => {
      if (pressed && (answered() || frames === cap)) {
        document.removeEventListener("keydown", onKey, { capture: true });
        resolve(frames);
        return;
      }
      frames += pressed ? 1 : 0;
      requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
  });
  Reflect.set(window, slot, counted);
}

// A key that opens a dialog waits until none is left; its Escape waits
// until the focus is inside the dialog.
async function readyFor(page: Page, key: string, answer: Answer): Promise<void> {
  if (answer === "dialog") {
    await expect(page.getByRole("dialog"), `dialogs in the DOM before ${key}`).toHaveCount(0);
  }
  if (answer === "returned") {
    const holding = page.locator('[role="dialog"]:focus-within');
    await expect(holding, `dialogs holding the focus before ${key}`).toHaveCount(1);
  }
}

async function framesToAnswer(page: Page, key: string, answer: Answer): Promise<number> {
  // A chord answers on its last key, not on the modifier that goes down first.
  const last = key.split("+").at(-1) ?? key;
  await page.evaluate(countFramesToAnswer, {
    key: last,
    answer,
    cap: FRAMES_CAP,
    slot: COUNT_SLOT,
    origin: ORIGIN_SLOT,
  });
  await page.keyboard.press(key);
  const frames = await page.evaluate((slot): unknown => Reflect.get(window, slot), COUNT_SLOT);
  if (typeof frames !== "number") {
    throw new Error("the page counted nothing");
  }
  return frames;
}

test("the keys of the long list answer before the next animation frame", async ({ page }) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await openInbox(page);
  await firstRow(page).focus();
  for (const [key, answer] of KEY_ANSWERS) {
    await readyFor(page, key, answer);
    const frames = await framesToAnswer(page, key, answer);
    expect(frames, `${key}: animation frames before its answer`).toBe(0);
  }
});
