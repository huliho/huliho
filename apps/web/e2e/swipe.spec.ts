// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import type { MailServer } from "./mail-answers";
import { THEMES, VIEWPORTS, axeOn } from "./sweep";
import {
  INBOX_PATH,
  WRITABLE_READ_INBOX,
  grid,
  openInbox,
  rowAt,
  settled,
  subjectOf,
  threadOf,
  updates,
} from "./thread-pane";

// The moves a finger sends on its way along a row.
const STEPS = 8;
// Where a finger lands, this far inside the row's edge it travels from.
const EDGE_PX = 24;
// Shares of the row's width: a quarter, short of the commit point and past it.
const QUARTER = 0.25;
const SHORT = 0.3;
const PAST = 0.6;
// How far a vertical drag travels up the list.
const SCROLL_PX = 240;

interface Point {
  x: number;
  y: number;
}

// A finger on the screen through the DevTools protocol, since
// Playwright's touchscreen only taps. It lands, moves in steps from
// where it stands and lifts.
interface Finger {
  down: (at: Point) => Promise<void>;
  moveTo: (to: Point) => Promise<void>;
  lift: () => Promise<void>;
}

async function fingerOn(page: Page): Promise<Finger> {
  const cdp = await page.context().newCDPSession(page);
  let here: Point = { x: 0, y: 0 };
  const send = async (
    type: "touchStart" | "touchMove" | "touchEnd",
    touchPoints: Point[],
  ): Promise<void> => {
    await cdp.send("Input.dispatchTouchEvent", { type, touchPoints });
  };
  return {
    down: async (at) => {
      here = at;
      await send("touchStart", [at]);
    },
    moveTo: async (to) => {
      const from = here;
      for (let step = 1; step <= STEPS; step += 1) {
        const share = step / STEPS;
        here = { x: from.x + (to.x - from.x) * share, y: from.y + (to.y - from.y) * share };
        await send("touchMove", [here]);
      }
    },
    lift: () => send("touchEnd", []),
  };
}

// The row at its place in the list, in any locale: the grid's name
// follows the locale.
function rowIn(page: Page, index: number) {
  return page.getByRole("grid").locator(`[aria-rowindex="${String(index)}"]`);
}

// Where a finger lands on a row: through its middle line, near the edge
// it travels from.
async function landing(
  page: Page,
  index: number,
  share: number,
): Promise<Point & { width: number }> {
  const box = await rowIn(page, index).boundingBox();
  if (box === null) {
    throw new Error(`row ${String(index)} is not on the screen`);
  }
  const x = share > 0 ? box.x + EDGE_PX : box.x + box.width - EDGE_PX;
  return { x, y: box.y + box.height / 2, width: box.width };
}

// A finger lands on the row and travels `share` of its width, to the
// right where the share is positive; it stays down.
async function drag(page: Page, index: number, share: number): Promise<Finger> {
  const from = await landing(page, index, share);
  const finger = await fingerOn(page);
  await finger.down(from);
  await finger.moveTo({ x: from.x + share * from.width, y: from.y });
  return finger;
}

function toast(page: Page, text: string) {
  return page.getByRole("dialog").filter({ hasText: text });
}

// What the ground behind a row says: its word, empty while the icon stands alone.
function groundOf(page: Page, index: number) {
  return rowIn(page, index).locator("[data-way]");
}

function swiping(page: Page) {
  return grid(page).locator("[data-swiping]");
}

// Watches the grid for the close-up mark; the call it returns reads the
// rows the mark has landed on so far, by their aria-rowindex.
async function closeUps(page: Page): Promise<() => Promise<number[]>> {
  const landed = await grid(page).evaluateHandle((element) => {
    const marked: number[] = [];
    new MutationObserver((records) => {
      for (const record of records) {
        if (record.target instanceof HTMLElement && record.oldValue === null) {
          marked.push(Number(record.target.getAttribute("aria-rowindex")));
        }
      }
    }).observe(element, {
      attributeFilter: ["data-closing"],
      attributeOldValue: true,
      subtree: true,
    });
    return marked;
  });
  return () => landed.jsonValue();
}

// How far the page scrolls sideways; zero when nothing sticks out.
function sidewaysOverflow(page: Page): Promise<number> {
  return page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
}

async function inbox(page: Page) {
  return openInbox(page, "right", INBOX_PATH, { mailboxes: WRITABLE_READ_INBOX });
}

// Nothing ran: the row stands home with its own thread, no toast shows
// and nothing reached the server.
async function nothingRan(page: Page, server: MailServer, index: number): Promise<void> {
  await expect(swiping(page)).toHaveCount(0);
  await expect(rowAt(page, index)).toContainText(subjectOf(index));
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page).toHaveURL(INBOX_PATH);
  expect(server.sets).toEqual([]);
}

// Every swipe runs on a phone's touchscreen.
test.use({ hasTouch: true, viewport: VIEWPORTS[0] });

test("a vertical drag scrolls the list and moves no row", async ({ page }) => {
  const { server } = await inbox(page);
  const from = await landing(page, 3, 1);
  const finger = await fingerOn(page);
  await finger.down(from);
  await finger.moveTo({ x: from.x, y: from.y - SCROLL_PX });
  await finger.lift();
  await expect.poll(() => grid(page).evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
  await nothingRan(page, server, 3);
});

test("short of half the row goes home: nothing sent, nothing opened", async ({ page }) => {
  const { server } = await inbox(page);
  const finger = await drag(page, 1, SHORT);
  await expect(groundOf(page, 1)).toHaveText("");
  await finger.lift();
  await nothingRan(page, server, 1);
});

test("a finger that leaves the row across its edge sends it home and runs nothing", async ({
  page,
}) => {
  const { server } = await inbox(page);
  const from = await landing(page, 2, 1);
  const finger = await drag(page, 2, PAST);
  await expect(groundOf(page, 2)).toHaveText("Archive");
  await finger.moveTo({ x: from.x + PAST * from.width, y: from.y + SCROLL_PX });
  await finger.lift();
  await nothingRan(page, server, 2);
});

test("toward the line's end the icon shows alone, then the word; a release past half archives and the rows close up; Undo puts it back", async ({
  page,
}) => {
  const { server } = await inbox(page);
  const short = await drag(page, 1, QUARTER);
  await expect(groundOf(page, 1)).toHaveText("");
  await settled(page);
  await expect.soft(rowAt(page, 1)).toHaveScreenshot("swipe-archive-quarter-light-phone.png");
  await short.lift();
  await expect(swiping(page)).toHaveCount(0);
  const held = await drag(page, 1, PAST);
  await expect(groundOf(page, 1)).toHaveText("Archive");
  expect(await sidewaysOverflow(page)).toBe(0);
  await settled(page);
  for (const theme of THEMES) {
    await page.emulateMedia({ colorScheme: theme });
    await expect.soft(rowAt(page, 1)).toHaveScreenshot(`swipe-archive-${theme}-phone.png`);
  }
  expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
  const closedUp = await closeUps(page);
  await held.lift();
  await expect(toast(page, "Archived.")).toBeVisible();
  await expect(rowAt(page, 1)).toContainText(subjectOf(2));
  expect(await closedUp()).toContain(1);
  await expect(grid(page).locator("[data-closing]")).toHaveCount(0);
  await expect(page).toHaveURL(INBOX_PATH);
  const members = threadOf(1).map((email) => email.id);
  await expect.poll(() => updates(server)).toEqual([members]);
  await toast(page, "Archived.").getByRole("button", { name: "Undo" }).tap();
  await expect(rowAt(page, 1)).toContainText(subjectOf(1));
});

test("toward the line's start a release past half deletes", async ({ page }) => {
  const { server } = await inbox(page);
  const held = await drag(page, 1, -PAST);
  await expect(groundOf(page, 1)).toHaveText("Delete");
  await settled(page);
  for (const theme of THEMES) {
    await page.emulateMedia({ colorScheme: theme });
    await expect.soft(rowAt(page, 1)).toHaveScreenshot(`swipe-delete-${theme}-phone.png`);
  }
  await held.lift();
  await expect(toast(page, "Deleted.")).toBeVisible();
  const members = threadOf(1).map((email) => email.id);
  await expect.poll(() => updates(server)).toEqual([members]);
  expect(server.corpus.emails.get(members[0] ?? "")?.mailboxId).toBe("mb-trash");
});

test("right to left a finger moving left archives, toward the line's end", async ({ page }) => {
  const { server } = await inbox(page);
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "en-XA");
  });
  await page.goto(INBOX_PATH);
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
  const held = await drag(page, 1, -PAST);
  await expect(groundOf(page, 1)).toHaveAttribute("data-way", "toEnd");
  await settled(page);
  await expect.soft(rowIn(page, 1)).toHaveScreenshot("swipe-en-XA-light-phone.png");
  await held.lift();
  const members = threadOf(1).map((email) => email.id);
  await expect.poll(() => updates(server)).toEqual([members]);
  expect(server.corpus.emails.get(members[0] ?? "")?.mailboxId).toBe("mb-archive");
});
