// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { firstRow, openLongInbox } from "../long-list";
import { INTERACTION_BUDGET_MS } from "../phone-profile";
import { VIEWPORTS } from "../sweep";

// Where the page keeps the measurement between the two evaluations.
const MEASUREMENT = "__keyToPaint";

// The time from the key going down to the first frame painted with the
// focus on another element, measured inside the page: a frame that
// finds the focus moved is the one that paints it. A focus that falls
// to the body is no answer. The listeners go in before the key, in an
// evaluation of their own, since a key sent beside one can land first.
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
        const active = document.activeElement;
        if (pressed > 0 && active !== null && active !== document.body && active !== before) {
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
  await openLongInbox(page);
  await firstRow(page).focus();
  for (const key of MEASURED_KEYS) {
    const elapsed = await keyToPaint(page, key);
    test
      .info()
      .annotations.push({ type: `${key} to paint`, description: `${elapsed.toFixed(1)} ms` });
    expect(elapsed, key).toBeLessThan(INTERACTION_BUDGET_MS);
  }
});
