// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { corpusFor, seedThread } from "./mail-corpus";
import { INTERACTION_BUDGET_MS, onPhoneProfile } from "./phone-profile";
import { VIEWPORTS } from "./sweep";
import { INBOX_PATH, READ_INBOX, cards, frameIn, openInbox, rowAt } from "./thread-pane";

const INBOX_ID = "mb-inbox";
// The longest the main thread may stay busy in one task while the
// message builds, so a tap in the middle of it still meets the INP budget.
const INP_BUDGET_MS = 200;
// The still lines of a thread or a body on its way.
const SKELETON = '[role="status"][aria-label="Loading…"]';
// Where the page keeps the measurement between the two evaluations.
const MEASUREMENT = "__openToPaint";
// The weight Gmail clips a message at, which senders build up to: as
// heavy as a newsletter commonly gets.
const NEWSLETTER_BYTES = 102 * 1024;
// A mail that names this many remote images, each with a text of its own.
const FLOOD_IMAGES = 20_000;
// The boxes a message draws for blocked images; the rest stay blank.
const BOXES = 100;
// The flood stands at its height about three seconds after the click on
// a desktop here and thirteen on the phone profile; a shared runner gets this long.
const FLOOD_SHOWN_MS = 20_000;

const PARAGRAPH_STYLE =
  "padding:0 24px 16px 24px;font-family:Helvetica,Arial,sans-serif;font-size:16px;line-height:24px;color:#333333";
const PARAGRAPH = `<tr><td align="left" valign="top" style="${PARAGRAPH_STYLE}">A paragraph of the story, written the way a newsletter writes it: a few sentences that set the scene, name the product and say why this week is the week to look at it once more.</td></tr>`;
const PARAGRAPHS_PER_STORY = 6;

// One story as senders write them: a table of its own, inline styles on
// every cell, a remote image above it and a button built from a table.
function story(index: number): string {
  const n = String(index);
  return `<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="border-collapse:collapse;background-color:#ffffff"><tr><td align="center" style="padding:0"><img src="https://cdn.example.test/story/${n}.jpg" width="600" height="240" alt="Story ${n}" style="display:block;width:100%;max-width:600px;height:auto;border:0"></td></tr><tr><td align="left" style="padding:24px 24px 8px 24px;font-family:Georgia,serif;font-size:22px;line-height:28px;color:#111111;font-weight:bold">Story ${n}: a headline that runs to a second line</td></tr>${PARAGRAPH.repeat(PARAGRAPHS_PER_STORY)}<tr><td align="left" style="padding:0 24px 32px 24px"><table role="presentation" cellpadding="0" cellspacing="0" border="0"><tr><td align="center" bgcolor="#1a5fb4" style="border-radius:4px;background-color:#1a5fb4"><a href="https://shop.example.test/story/${n}?utm_source=newsletter&utm_medium=email" style="display:inline-block;padding:12px 24px;font-family:Helvetica,Arial,sans-serif;font-size:16px;line-height:20px;color:#ffffff;text-decoration:none;font-weight:bold">Read the story</a></td></tr></table></td></tr></table>`;
}

// A newsletter of that weight: stories until it is reached.
function heavyNewsletter(): string {
  const stories: string[] = [];
  for (let bytes = 0; bytes < NEWSLETTER_BYTES; bytes += stories.at(-1)?.length ?? 0) {
    stories.push(story(stories.length));
  }
  return stories.join("");
}

function imageFlood(): string {
  return Array.from({ length: FLOOD_IMAGES }, (_, index) => {
    const n = String(index);
    return `<img src="https://cdn.example.test/flood/${n}.png" width="40" height="20" alt="Image ${n}">`;
  }).join("");
}

// What opening the message cost: the time to the first frame painted
// with the skeleton, the time to the frame at its height and the
// longest task of the main thread in between.
interface Painted {
  skeleton: number;
  frame: number;
  longestTask: number;
}

function isPainted(value: unknown): value is Painted {
  return (
    typeof value === "object" &&
    value !== null &&
    ["skeleton", "frame", "longestTask"].every((key) => typeof Reflect.get(value, key) === "number")
  );
}

// Measured inside the page, from the click going down. A frame that
// finds the skeleton in place is the one that paints it. The long
// tasks come from the browser's own record; a task under its 50 ms
// floor counts as none. The listeners go in before the click, in an
// evaluation of their own.
async function openToPaint(page: Page, rowIndex: number): Promise<Painted> {
  await page.evaluate(
    ({ slot, skeletonSelector }) => {
      const measured = new Promise<Painted>((resolve) => {
        let pressed = 0;
        let skeleton = 0;
        const tasks: PerformanceEntry[] = [];
        const observer = new PerformanceObserver((list) => {
          tasks.push(...list.getEntries());
        });
        observer.observe({ type: "longtask" });
        document.addEventListener(
          "pointerdown",
          () => {
            pressed = performance.now();
          },
          { capture: true, once: true },
        );
        const frame = (now: number): void => {
          if (pressed > 0 && skeleton === 0 && document.querySelector(skeletonSelector) !== null) {
            skeleton = now - pressed;
          }
          if (pressed === 0 || document.querySelector("iframe:not([data-loading])") === null) {
            requestAnimationFrame(frame);
            return;
          }
          tasks.push(...observer.takeRecords());
          observer.disconnect();
          const since = tasks.filter((task) => task.startTime + task.duration >= pressed);
          resolve({
            skeleton,
            frame: now - pressed,
            longestTask: Math.max(0, ...since.map((task) => task.duration)),
          });
        };
        requestAnimationFrame(frame);
      });
      Reflect.set(window, slot, measured);
    },
    { slot: MEASUREMENT, skeletonSelector: SKELETON },
  );
  await rowAt(page, rowIndex).click();
  const painted = await page.evaluate((slot): unknown => Reflect.get(window, slot), MEASUREMENT);
  if (!isPainted(painted)) {
    throw new Error("the page measured nothing");
  }
  return painted;
}

test("opening a heavy newsletter paints the skeleton inside the interaction budget and builds the message without a task past the INP budget on the phone profile", async ({
  page,
}) => {
  await page.setViewportSize(VIEWPORTS[0]);
  const corpus = corpusFor(READ_INBOX);
  seedThread(corpus, INBOX_ID, [{ seen: true, body: { html: heavyNewsletter() } }]);
  await onPhoneProfile(page, async ({ benchmarkIndex, slowdown }) => {
    await openInbox(page, "right", INBOX_PATH, { mail: { corpus } });
    // Nothing loads before the click, so the skeleton the page finds is the click's.
    await expect(page.locator(SKELETON)).toHaveCount(0);
    const painted = await openToPaint(page, 1);
    // The figures go to stdout first, so the log carries them whatever the outcome.
    console.log(
      `phone open: benchmark index ${benchmarkIndex.toFixed(0)}, CPU slowed ${slowdown.toFixed(1)}x, skeleton ${painted.skeleton.toFixed(1)} ms, frame ${painted.frame.toFixed(1)} ms, longest task ${painted.longestTask.toFixed(1)} ms`,
    );
    expect(painted.skeleton, "skeleton").toBeGreaterThan(0);
    expect(painted.skeleton, "skeleton").toBeLessThan(INTERACTION_BUDGET_MS);
    expect(painted.longestTask, "longest task").toBeLessThan(INP_BUDGET_MS);
  });
});

test("a mail of twenty thousand remote images shows in seconds, with a box for the first hundred and the rest blank", async ({
  page,
}) => {
  await page.setViewportSize(VIEWPORTS[1]);
  const corpus = corpusFor(READ_INBOX);
  seedThread(corpus, INBOX_ID, [{ seen: true, body: { html: imageFlood() } }]);
  await openInbox(page, "right", INBOX_PATH, { mail: { corpus } });
  const pressed = Date.now();
  await rowAt(page, 1).click();
  const frame = frameIn(cards(page).last());
  await expect(frame).not.toHaveAttribute("data-loading", { timeout: FLOOD_SHOWN_MS });
  console.log(`image flood: shown ${String(Date.now() - pressed)} ms after the click`);
  const message = cards(page).last().frameLocator("iframe");
  await expect(message.locator("img")).toHaveCount(FLOOD_IMAGES);
  await expect(message.locator('img[src^="data:image/svg+xml"]')).toHaveCount(BOXES);
});
