// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Locator, Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { NEWSLETTER_SENDER } from "./mail-bodies";
import { mockPreferences } from "./preference-mocks";
import { mockSignedIn } from "./session-mocks";
import {
  INBOX_PATH,
  bodiesLanded,
  cards,
  frameIn,
  heightOf,
  openInbox,
  pathOf,
  rowAt,
  rowFrom,
} from "./thread-pane";

const PAGE = "/settings/appearance";
// Body text at the default root of 16 px: type-2 with the comfortable leading.
const DEFAULT_TYPE = "13px";
const DEFAULT_LEADING = "18.85px";
// The root's size at the largest step, from the browser's own 16 px.
const LARGER_ROOT = "20px";
// Body text at the largest step: type-2 at that root.
const LARGER_TYPE = "16.25px";
// The used leading of that text at the loosest step, 1.75.
const LOOSE_LEADING = "28.4375px";
// The same text in compact rows at the loosest step: 1.35 plus 0.3.
const COMPACT_LOOSE_LEADING = "26.8125px";
// How long the server takes to answer the words on record on the next load.
const SLOW_ANSWER_MS = 1_500;
// The largest type with the loosest leading, as the words on record say it.
const SCALED = { fontSize: "larger", lineHeight: "loose" };
// The designed row heights at a 16 px root, which hold at the default steps.
const DESIGNED_ROWS = [
  { density: "comfortable", rowPx: 52 },
  { density: "compact", rowPx: 40 },
];
// A row's layout height is rounded to a fraction of a pixel; the next row starts there.
const ROW_EDGE_DIGITS = 1;

interface Edges {
  top: number;
  bottom: number;
}

async function edgesOf(locator: Locator): Promise<Edges> {
  const box = await locator.boundingBox();
  if (box === null) {
    throw new Error("the element has no box");
  }
  return { top: box.y, bottom: box.y + box.height };
}

// The first row, its second line (the last of the cell's own text runs,
// after the sender's) and the row under it.
async function firstRowEdges(page: Page): Promise<{ row: Edges; line: Edges; next: Edges }> {
  const row = rowAt(page, 1);
  const line = row.locator("[role='gridcell'] > [dir='auto']").last();
  return {
    row: await edgesOf(row),
    line: await edgesOf(line),
    next: await edgesOf(rowAt(page, 2)),
  };
}

function root(page: Page): Locator {
  return page.locator("html");
}

function checked(page: Page, group: string): Locator {
  return page.getByRole("radiogroup", { name: group }).getByRole("radio", { checked: true });
}

// A whole word: "Large" must not reach "Larger".
async function pick(page: Page, group: string, word: string): Promise<void> {
  await page
    .getByRole("radiogroup", { name: group })
    .getByRole("radio", { name: word, exact: true })
    .click();
}

// The type size and the leading as the engine resolved them, around the
// frame and at its root, so the two can be read against each other.
async function typeAround(card: Locator): Promise<{ card: string; frame: string }> {
  return frameIn(card).evaluate((element) => {
    const page = element instanceof HTMLIFrameElement ? element.contentDocument : null;
    const around = element.parentElement === null ? null : getComputedStyle(element.parentElement);
    return {
      card: around === null ? "" : `${around.fontSize} ${around.lineHeight}`,
      frame:
        page === null
          ? ""
          : `${getComputedStyle(page.documentElement).fontSize} ${getComputedStyle(page.body).lineHeight}`,
    };
  });
}

test("the type settings apply at once, reach the server and stand on the next load before it answers", async ({
  page,
}) => {
  await mockSignedIn(page);
  const { writes } = await mockPreferences(page);
  await page.goto(PAGE);
  await expect(root(page)).toHaveAttribute("data-font-size", "default");
  await expect(root(page)).toHaveAttribute("data-line-height", "default");
  await pick(page, "Font size", "Larger");
  await expect(root(page)).toHaveAttribute("data-font-size", "larger");
  await expect(root(page)).toHaveCSS("font-size", LARGER_ROOT);
  await pick(page, "Line height", "Loose");
  await expect(root(page)).toHaveAttribute("data-line-height", "loose");
  await expect(page.locator("body")).toHaveCSS("line-height", LOOSE_LEADING);
  // The step rides the density's own leading.
  await pick(page, "Density", "Compact");
  await expect(page.locator("body")).toHaveCSS("line-height", COMPACT_LOOSE_LEADING);
  await expect
    .poll(() => writes)
    .toEqual([
      { key: "fontSize", value: "larger" },
      { key: "lineHeight", value: "loose" },
      { key: "density", value: "compact" },
    ]);
  // The next load: the device's memory stands while the server is still asked.
  await page.route("**/api/preferences", async (route) => {
    await new Promise((resolve) => {
      setTimeout(resolve, SLOW_ANSWER_MS);
    });
    await route.fallback();
  });
  await page.reload();
  await expect(page.getByLabel("Loading…")).toBeVisible();
  await expect(root(page)).toHaveAttribute("data-font-size", "larger");
  await expect(root(page)).toHaveAttribute("data-line-height", "loose");
  await expect(checked(page, "Font size")).toHaveText("Larger");
  await expect(checked(page, "Line height")).toHaveText("Loose");
});

test("the frame's text follows the font size and the line height of the card around it", async ({
  page,
}) => {
  const path = pathOf(rowFrom(NEWSLETTER_SENDER));
  await openInbox(page, "right", path);
  await bodiesLanded(page);
  const plain = await typeAround(cards(page).last());
  expect(plain.frame).toBe(plain.card);
  expect(plain.card).toBe(`${DEFAULT_TYPE} ${DEFAULT_LEADING}`);
  await openInbox(page, "right", path, {
    preferences: { fontSize: "larger", lineHeight: "loose" },
  });
  await bodiesLanded(page);
  const scaled = await typeAround(cards(page).last());
  expect(scaled.frame).toBe(scaled.card);
  expect(scaled.card).toBe(`${LARGER_TYPE} ${LOOSE_LEADING}`);
});

test("the list rows grow with the font size and the line height, so the second line ends inside the row", async ({
  page,
}) => {
  for (const { density, rowPx } of DESIGNED_ROWS) {
    await test.step(`${density} rows`, async () => {
      await openInbox(page, "right", INBOX_PATH, { preferences: { density } });
      await expect.poll(() => heightOf(rowAt(page, 1))).toBe(rowPx);
      await openInbox(page, "right", INBOX_PATH, { preferences: { density, ...SCALED } });
      await expect.poll(() => heightOf(rowAt(page, 1))).toBeGreaterThan(rowPx);
      const { row, line, next } = await firstRowEdges(page);
      expect(line.top).toBeGreaterThanOrEqual(row.top);
      expect(line.bottom).toBeLessThanOrEqual(row.bottom);
      expect(next.top).toBeCloseTo(row.bottom, ROW_EDGE_DIGITS);
    });
  }
});

test("Show as sent leaves a light message on its own sheet in the dark theme; its head still adapts it", async ({
  page,
}) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await openInbox(page, "right", INBOX_PATH);
  const { writes } = await mockPreferences(page, { readingPane: "right" });
  await page.goto(PAGE);
  await pick(page, "Dark mode for messages", "Show as sent");
  await expect.poll(() => writes).toEqual([{ key: "darkMail", value: "original" }]);
  await page.goto(pathOf(rowFrom(NEWSLETTER_SENDER)));
  await bodiesLanded(page);
  const card = cards(page).last();
  await expect(frameIn(card)).toHaveAttribute("data-canvas", "as-sent");
  const revert = card.getByRole("button", { name: "Adapt colors" });
  await expect(revert).toHaveAttribute("data-pressed", "true");
  await revert.click();
  await expect(frameIn(card)).not.toHaveAttribute("data-canvas", "as-sent");
  await expect(card.getByRole("button", { name: "Show original colors" })).toBeVisible();
});
