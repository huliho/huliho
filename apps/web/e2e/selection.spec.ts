// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Locator, Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { THEMES, VIEWPORTS, axeOn } from "./sweep";
import {
  INBOX_PATH,
  WRITABLE_READ_INBOX,
  grid,
  openInbox,
  pane,
  rowAt,
  settled,
  subjectOf,
  threadOf,
  updates,
} from "./thread-pane";

const TRASH_PATH = "/mail/acc-1/mb-trash";
// The narrowest screen the design supports.
const NARROW_PHONE = { width: 320, height: 568 };
// A touch rests on a row past the long press before it lifts.
const LONG_PRESS_HOLD_MS = 700;

function toast(page: Page, text: string) {
  return page.getByRole("dialog").filter({ hasText: text });
}

function bar(page: Page) {
  return page.getByRole("toolbar");
}

// The names of the bar's buttons, in order.
async function barButtons(page: Page): Promise<(string | null)[]> {
  const buttons = await bar(page).getByRole("button").all();
  return Promise.all(buttons.map((button) => button.getAttribute("aria-label")));
}

function checkOf(page: Page, index: number) {
  return rowAt(page, index).getByRole("checkbox", { name: "Select conversation" });
}

// Whether the element's box lies inside a screen this wide.
async function inside(locator: Locator, width: number): Promise<boolean> {
  const box = await locator.boundingBox();
  return box !== null && box.x >= 0 && box.x + box.width <= width;
}

// How far the page scrolls sideways; zero when nothing sticks out.
function sidewaysOverflow(page: Page): Promise<number> {
  return page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
}

// Whether the focus stands on a row of the grid.
function focusOnRow(page: Page): Promise<boolean> {
  return page.evaluate(() => {
    const row = document.activeElement?.closest('[role="row"]');
    return row?.closest('[role="grid"]')?.getAttribute("aria-label") === "Conversations";
  });
}

async function inbox(page: Page, path: string = INBOX_PATH) {
  return openInbox(page, "right", path, { mailboxes: WRITABLE_READ_INBOX });
}

test("x picks rows, the bar counts them and archives them in one move with one toast; z puts them back", async ({
  page,
}) => {
  const { server } = await inbox(page);
  await rowAt(page, 1).focus();
  await page.keyboard.press("x");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("x");
  await expect(bar(page)).toHaveAccessibleName("2 selected");
  await expect(grid(page)).toHaveAttribute("aria-multiselectable", "true");
  await expect(rowAt(page, 1)).toHaveAttribute("aria-selected", "true");
  await expect(rowAt(page, 3)).toHaveAttribute("aria-selected", "false");
  expect(await barButtons(page)).toEqual([
    "Archive",
    "Delete",
    "Mark as spam",
    "Mark as unread",
    "Move to…",
    "Close selection",
  ]);
  await settled(page);
  for (const theme of THEMES) {
    await page.emulateMedia({ colorScheme: theme });
    await expect.soft(grid(page)).toHaveScreenshot(`selection-bar-${theme}-desktop.png`);
  }
  expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
  const members = [...threadOf(1), ...threadOf(2)].map((email) => email.id);
  await page.keyboard.press("e");
  await expect(toast(page, "2 conversations archived.")).toBeVisible();
  await expect(bar(page)).toHaveCount(0);
  await expect(rowAt(page, 1)).toContainText(subjectOf(3));
  await expect(rowAt(page, 1)).toBeFocused();
  await expect.poll(() => updates(server)).toEqual([members]);
  await page.keyboard.press("z");
  await expect(rowAt(page, 1)).toContainText(subjectOf(1));
  await expect(rowAt(page, 2)).toContainText(subjectOf(2));
});

test("a click on the hovered row's check picks it and Shift with a click picks the run; Escape closes the mode before the thread", async ({
  page,
}) => {
  await inbox(page);
  await rowAt(page, 2).hover();
  await checkOf(page, 2).click();
  await expect(bar(page)).toHaveAccessibleName("1 selected");
  await rowAt(page, 5).click({ modifiers: ["Shift"] });
  await expect(bar(page)).toHaveAccessibleName("4 selected");
  await expect(checkOf(page, 6)).toHaveAttribute("aria-checked", "false");
  await rowAt(page, 7).getByText(subjectOf(7), { exact: true }).click();
  await expect(pane(page).getByRole("heading", { level: 2 })).toHaveText(subjectOf(7));
  await expect(bar(page)).toHaveAccessibleName("4 selected");
  await page.keyboard.press("Escape");
  await expect(bar(page)).toHaveCount(0);
  await expect(pane(page).getByRole("heading", { level: 2 })).toHaveText(subjectOf(7));
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(INBOX_PATH);
});

test("in Trash the bar deletes forever and leaves archive out", async ({ page }) => {
  await inbox(page, TRASH_PATH);
  await rowAt(page, 1).focus();
  await page.keyboard.press("x");
  await expect(bar(page)).toBeVisible();
  expect(await barButtons(page)).toEqual([
    "Delete forever",
    "Mark as spam",
    "Mark as unread",
    "Move to…",
    "Close selection",
  ]);
  await bar(page).getByRole("button", { name: "Delete forever" }).hover();
  await expect(page.locator("[data-side][data-open]").last()).toHaveText(/Delete forever\s*#/);
  await settled(page);
  await expect.soft(grid(page)).toHaveScreenshot("selection-bar-trash-light-desktop.png");
});

test("the bar and the checks read right to left in the pseudo-locale", async ({ page }) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await inbox(page);
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "en-XA");
  });
  await page.goto(INBOX_PATH);
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
  const first = page.getByRole("grid").locator('[aria-rowindex="1"]');
  await first.focus();
  await page.keyboard.press("x");
  await expect(bar(page)).toBeVisible();
  await settled(page);
  await expect.soft(page.getByRole("grid")).toHaveScreenshot("selection-en-XA-light-desktop.png");
});

// A touch that rests on the row's middle past the long press, through
// the DevTools protocol, since Playwright's touchscreen only taps.
async function longPress(page: Page, index: number): Promise<void> {
  const box = await rowAt(page, index).boundingBox();
  const point = {
    x: (box?.x ?? 0) + (box?.width ?? 0) / 2,
    y: (box?.y ?? 0) + (box?.height ?? 0) / 2,
  };
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [point] });
  await page.waitForTimeout(LONG_PRESS_HOLD_MS);
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
}

// The first two rows picked through the mailbox menu's Select and their checks.
async function pickTwo(page: Page): Promise<void> {
  await page.getByRole("heading", { level: 1 }).getByRole("button").tap();
  await page.getByRole("menuitem", { name: "Select" }).tap();
  await checkOf(page, 1).tap();
  await checkOf(page, 2).tap();
  await expect(bar(page)).toHaveAccessibleName("2 selected");
}

test.describe("on a touchscreen", () => {
  test.use({ hasTouch: true, viewport: VIEWPORTS[0] });

  test("a long press picks a row without opening it, a tap picks another and the bar keeps spam behind More actions", async ({
    page,
  }) => {
    await inbox(page);
    await longPress(page, 2);
    await expect(bar(page)).toHaveAccessibleName("1 selected");
    await expect(page).toHaveURL(INBOX_PATH);
    await rowAt(page, 4).tap();
    await expect(bar(page)).toHaveAccessibleName("2 selected");
    await expect(page).toHaveURL(INBOX_PATH);
    expect(await barButtons(page)).toEqual([
      "Archive",
      "Delete",
      "Mark as unread",
      "More actions",
      "Close selection",
    ]);
    await settled(page);
    for (const theme of THEMES) {
      await page.emulateMedia({ colorScheme: theme });
      await expect.soft(page).toHaveScreenshot(`selection-touch-${theme}-phone.png`);
    }
    await bar(page).getByRole("button", { name: "More actions" }).tap();
    await expect(page.getByRole("menuitem", { name: "Mark as spam" })).toBeVisible();
  });

  test("the mailbox's name offers Select, which brings the checks out with nothing picked", async ({
    page,
  }) => {
    await inbox(page);
    await page.getByRole("heading", { level: 1 }).getByRole("button").tap();
    await page.getByRole("menuitem", { name: "Select" }).tap();
    await expect(bar(page)).toHaveAccessibleName("0 selected");
    expect(await barButtons(page)).toEqual(["Close selection"]);
    await expect(checkOf(page, 1)).toHaveAttribute("aria-checked", "false");
    await checkOf(page, 3).tap();
    await expect(bar(page)).toHaveAccessibleName("1 selected");
    expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
  });

  test("an action behind More actions takes the picks in one move and leaves the focus on a row", async ({
    page,
  }) => {
    const { server } = await inbox(page);
    await pickTwo(page);
    await bar(page).getByRole("button", { name: "More actions" }).tap();
    await page.getByRole("menuitem", { name: "Mark as spam" }).tap();
    const members = [...threadOf(1), ...threadOf(2)].map((email) => email.id);
    await expect.poll(() => updates(server)).toEqual([members]);
    await expect(bar(page)).toHaveCount(0);
    await expect.poll(() => focusOnRow(page)).toBe(true);
  });
});

test.describe("on the narrowest touchscreen", () => {
  test.use({ hasTouch: true, viewport: NARROW_PHONE });

  test("the bar with two rows picked stays inside the screen and nothing scrolls sideways", async ({
    page,
  }) => {
    await inbox(page);
    await pickTwo(page);
    expect(await inside(bar(page), NARROW_PHONE.width)).toBe(true);
    const close = bar(page).getByRole("button", { name: "Close selection" });
    expect(await inside(close, NARROW_PHONE.width)).toBe(true);
    expect(await sidewaysOverflow(page)).toBe(0);
  });
});
