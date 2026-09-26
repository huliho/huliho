// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { AxeBuilder } from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { mockAccounts } from "./account-mocks";
import { FIXED_NOW } from "./mail-corpus";
import { mockMail } from "./mail-mocks";
import { mockPreferences } from "./preference-mocks";
import { mockSignedIn } from "./session-mocks";
import { THEMES, VIEWPORTS, WCAG_TAGS } from "./sweep";
import {
  CARDS_IN_SIGHT,
  INBOX_PATH,
  LONG_ROW,
  READ_INBOX,
  ROWS,
  SINGLE_ROW,
  directionOf,
  grid,
  heightOf,
  openInbox,
  pane,
  rowAt,
  screenOf,
  settled,
  subjectOf,
  threadIdOf,
  threadOf,
  threadSizeOf,
} from "./thread-pane";
import type { Position } from "./thread-pane";

// The tablet reference width, where the pane below the list gets its touch band.
const TABLET = { name: "tablet", width: 834, height: 1112 };
// The comfortable row and toolbar heights, which size the list below the pane.
const ROW_PX = 52;
const TOOLBAR_PX = 44;
// The same two at the touch density a touchscreen applies.
const TOUCH_ROW_PX = 56;
const TOUCH_TOOLBAR_PX = 56;
// The key-hint strip at the foot of the list, counted in the list's
// height beside the toolbar, so the rows above the pane stay whole.
const FOOT_PX = 36;
const TOUCH_FOOT_PX = 40;
// The seam's hit area on a touchscreen, its band in the flow and the grip on it.
const HIT_TARGET_PX = 44;
const TOUCH_BAND_PX = 12;
const GRIP_WIDTH_PX = 32;
const LIST_DEFAULT_ROWS = 6;
const LIST_MIN_ROWS = 4;
const PANE_MIN_HEIGHT_PX = 216;
// The most a card may span on a screen of its own; a bounding box
// carries float noise below a hundredth of a pixel.
const CARD_MAX_WIDTH_PX = 760;
const SUBPIXEL_PX = 0.01;
const DRAG_PX = 60;

test("Enter opens the row's thread beside the list with the focus on its title; Escape returns to the row", async ({
  page,
}) => {
  await openInbox(page);
  await expect(pane(page)).toContainText("Select a conversation.");
  await rowAt(page, 1).focus();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-inbox\/mb-inbox-t\d+$/);
  const title = pane(page).getByRole("heading", { level: 2, name: subjectOf(2) });
  await expect(title).toBeFocused();
  await expect(rowAt(page, 2)).toHaveAttribute("aria-selected", "true");
  await expect(rowAt(page, 1)).not.toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-inbox$/);
  await expect(rowAt(page, 2)).toBeFocused();
  await expect(pane(page)).toContainText("Select a conversation.");
  // Closing went back over the entry the open pushed, so forward reopens it.
  await page.goForward();
  await page.waitForURL(`**/mail/acc-1/mb-inbox/${threadIdOf(2)}`);
  await expect(rowAt(page, 2)).toHaveAttribute("aria-selected", "true");
  // Another row while a thread is open replaces the entry, so one back closes both.
  await rowAt(page, 3).click();
  await page.waitForURL(`**/mail/acc-1/mb-inbox/${threadIdOf(3)}`);
  await page.goBack();
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-inbox$/);
  await expect(pane(page)).toContainText("Select a conversation.");
});

test("a thread reached by its address returns the focus to its own row and the mailbox takes its place", async ({
  page,
}) => {
  await openInbox(page, "right", `${INBOX_PATH}/${threadIdOf(3)}`);
  await expect(pane(page).getByRole("heading", { level: 2 })).toHaveText(subjectOf(3));
  await expect(rowAt(page, 3)).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-inbox$/);
  await expect(rowAt(page, 3)).toBeFocused();
  // Nothing to go back over: the mailbox took the thread's place in the history.
  await page.goForward();
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-inbox$/);
  // A tab that starts at a thread's address and opens another row stays that way.
  await page.goto(`${INBOX_PATH}/${threadIdOf(3)}`);
  await rowAt(page, 2).click();
  await page.waitForURL(`**${INBOX_PATH}/${threadIdOf(2)}`);
  await expect(rowAt(page, 2)).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-inbox$/);
  await expect(rowAt(page, 2)).toBeFocused();
  await page.goForward();
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-inbox$/);
});

test("o and a click open a row; the cards show who wrote when and the older ones wait behind a button", async ({
  page,
}) => {
  await openInbox(page);
  await rowAt(page, 1).focus();
  await page.keyboard.press("o");
  await expect(pane(page).getByRole("heading", { level: 2 })).toHaveText(subjectOf(1));
  await rowAt(page, LONG_ROW).click();
  await expect(pane(page).getByRole("heading", { level: 2 })).toHaveText(subjectOf(LONG_ROW));
  await expect(rowAt(page, LONG_ROW)).toHaveAttribute("aria-selected", "true");
  const size = threadSizeOf(LONG_ROW);
  const hidden = size - CARDS_IN_SIGHT;
  await expect(pane(page).getByText(`${String(size)} messages`)).toBeVisible();
  const older = pane(page).getByRole("button", { name: /older message/ });
  await expect(older).toHaveText(`Show ${String(hidden)} older message${hidden === 1 ? "" : "s"}`);
  const cards = pane(page).getByRole("listitem");
  await expect(cards).toHaveCount(CARDS_IN_SIGHT);
  await expect(cards.nth(2).getByRole("button").first()).toHaveAttribute("aria-expanded", "true");
  await expect(cards.nth(2).getByText("to Mira")).toBeVisible();
  await expect(cards.nth(0).getByRole("button").first()).toHaveAttribute("aria-expanded", "false");
  await older.click();
  await expect(cards).toHaveCount(size);
  await expect(cards.first().getByRole("button").first()).toBeFocused();
  await pane(page).getByRole("button", { name: "Show all recipients" }).click();
  await expect(pane(page).getByText("mira@example.com")).toBeVisible();
  await pane(page).getByRole("button", { name: "Close conversation" }).click();
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-inbox$/);
  await expect(rowAt(page, LONG_ROW)).toBeFocused();
});

test("below the list the seam turns, moves by rows and keeps its place across a reload", async ({
  page,
}) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await openInbox(page, "bottom");
  const main = page.getByRole("main");
  const seam = page.getByRole("separator", { name: "Resize the list" });
  const fixed = TOOLBAR_PX + FOOT_PX;
  const defaultHeight = fixed + LIST_DEFAULT_ROWS * ROW_PX;
  await expect(seam).toHaveAttribute("aria-orientation", "horizontal");
  await expect(seam).toHaveAttribute("aria-valuenow", String(defaultHeight));
  await expect(seam).toHaveAttribute("aria-valuemin", String(fixed + LIST_MIN_ROWS * ROW_PX));
  await expect(seam).toHaveAttribute(
    "aria-valuemax",
    String(VIEWPORTS[1].height - PANE_MIN_HEIGHT_PX),
  );
  expect(await heightOf(main)).toBe(defaultHeight);
  // The rows above the seam are whole: the key-hint strip takes none of them.
  expect(await heightOf(grid(page))).toBeGreaterThanOrEqual(LIST_DEFAULT_ROWS * ROW_PX);
  await rowAt(page, SINGLE_ROW).click();
  await expect(pane(page).getByRole("heading", { level: 2 })).toHaveText(subjectOf(SINGLE_ROW));
  const box = await seam.boundingBox();
  if (box === null) {
    throw new Error("the seam has no box");
  }
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x, y + DRAG_PX, { steps: 4 });
  await page.mouse.up();
  expect(await heightOf(main)).toBe(defaultHeight + DRAG_PX);
  await page.reload();
  await expect(rowAt(page, 1)).toBeVisible();
  expect(await heightOf(main)).toBe(defaultHeight + DRAG_PX);
  await seam.focus();
  await page.keyboard.press("ArrowUp");
  expect(await heightOf(main)).toBe(defaultHeight + DRAG_PX - ROW_PX);
  await page.keyboard.press("Home");
  expect(await heightOf(main)).toBe(fixed + LIST_MIN_ROWS * ROW_PX);
  expect(await heightOf(grid(page))).toBeGreaterThanOrEqual(LIST_MIN_ROWS * ROW_PX);
  await page.keyboard.press("End");
  expect(await heightOf(pane(page))).toBeGreaterThanOrEqual(PANE_MIN_HEIGHT_PX);
  await page.keyboard.press("Enter");
  expect(await heightOf(main)).toBe(defaultHeight);
  expect(await page.evaluate(() => localStorage.getItem("huliho-list-height"))).toBeNull();
});

test("with the pane off the list fills the room and a thread opens as a screen of its own", async ({
  page,
}) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await openInbox(page, "off");
  await expect(page.getByRole("complementary")).toHaveCount(0);
  await expect(page.getByRole("separator")).toHaveCount(0);
  const main = page.getByRole("main");
  const wide = (await main.boundingBox())?.width ?? 0;
  expect(wide).toBeGreaterThan(VIEWPORTS[1].width / 2);
  await rowAt(page, 1).focus();
  await page.keyboard.press("Enter");
  const screen = screenOf(page);
  await expect(screen.getByRole("heading", { level: 1, name: subjectOf(1) })).toBeFocused();
  await expect(main).toHaveAttribute("inert", "");
  const card = screen.getByRole("listitem").last();
  expect((await card.boundingBox())?.width ?? 0).toBeLessThanOrEqual(
    CARD_MAX_WIDTH_PX + SUBPIXEL_PX,
  );
  await expect(page.getByRole("tree", { name: "Mailboxes" })).toBeVisible();
  await screen.getByRole("button", { name: "Back to Inbox" }).click();
  await expect(screenOf(page)).toHaveCount(0);
  await expect(rowAt(page, 1)).toBeFocused();
});

test("a phone opens the thread as a screen and the way back has no key", async ({ page }) => {
  await page.setViewportSize(VIEWPORTS[0]);
  await openInbox(page);
  await rowAt(page, 1).click();
  const screen = screenOf(page);
  await expect(screen.getByRole("heading", { level: 1, name: subjectOf(1) })).toBeVisible();
  await expect(screen.getByRole("button", { name: "Back to Inbox" })).not.toContainText("Esc");
  await page.goBack();
  await expect(screenOf(page)).toHaveCount(0);
  await expect(rowAt(page, 1)).toBeVisible();
  // The way back in the toolbar goes back too, so the browser's history reads mailbox, thread.
  await page.goForward();
  await screenOf(page).getByRole("button", { name: "Back to Inbox" }).click();
  await expect(screenOf(page)).toHaveCount(0);
  await page.goForward();
  await expect(screenOf(page).getByRole("heading", { level: 1, name: subjectOf(1) })).toBeVisible();
});

test("the pane reads in Dutch and in the pseudo-locale", async ({ page }) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await mockSignedIn(page);
  await mockMail(page, READ_INBOX);
  await mockAccounts(page, ROWS);
  await mockPreferences(page, { locale: "nl" });
  await page.clock.setFixedTime(FIXED_NOW);
  await page.goto("/mail/acc-1/mb-inbox");
  await page
    .getByRole("grid", { name: "Gesprekken" })
    .locator(`[aria-rowindex="${String(LONG_ROW)}"]`)
    .click();
  const gesprek = page.getByRole("complementary", { name: "Gesprek" });
  await expect(gesprek.getByRole("button", { name: /^Toon .* ouder/ })).toBeVisible();
  await expect(gesprek.getByText("aan Mira")).toBeVisible();
  await expect(gesprek.getByRole("button", { name: "Alle ontvangers tonen" })).toBeVisible();
  await expect(gesprek.getByRole("button", { name: "Gesprek sluiten" })).toBeVisible();
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "en-XA");
  });
  await page.goto("/mail/acc-1/mb-inbox");
  const longRow = page.getByRole("grid").locator(`[aria-rowindex="${String(LONG_ROW)}"]`);
  await longRow.click();
  const pseudo = page.getByRole("complementary");
  const title = pseudo.getByRole("heading", { level: 2 });
  await expect(title).toHaveText(subjectOf(LONG_ROW));
  await expect(pseudo.getByRole("button", { name: /óldér mésságé/ })).toBeVisible();
  // The pseudo-locale reads right to left, so the pane at the inline end sits left of the list.
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
  const main = (await page.getByRole("main").boundingBox()) ?? { x: 0, width: 0 };
  const aside = (await pseudo.boundingBox()) ?? { x: 0, width: 0 };
  expect(aside.x + aside.width).toBeLessThanOrEqual(main.x);
  // Latin mail keeps its own direction inside the right-to-left page: the
  // row's subject, the pane's title and a card's sender read left to right.
  expect(await directionOf(pseudo)).toBe("rtl");
  const newest = threadOf(LONG_ROW).at(-1);
  const sender = pseudo
    .getByRole("listitem")
    .last()
    .getByText(newest?.from.name ?? "", { exact: true });
  const subject = longRow.getByText(subjectOf(LONG_ROW), { exact: true });
  for (const own of [subject, title, sender]) {
    expect(await directionOf(own)).toBe("ltr");
  }
  await settled(page);
  await expect.soft(page).toHaveScreenshot("thread-en-XA-light-desktop.png");
});

interface Drawn {
  name: string;
  readingPane: Position;
  viewport: { name: string; width: number; height: number };
}

// The pane beside the list, below it and off at the desktop width and
// the screen on a phone; the tablet width runs on a touchscreen below.
const DRAWN: Drawn[] = [
  { name: "right", readingPane: "right", viewport: VIEWPORTS[1] },
  { name: "bottom", readingPane: "bottom", viewport: VIEWPORTS[1] },
  { name: "off", readingPane: "off", viewport: VIEWPORTS[1] },
  { name: "phone", readingPane: "right", viewport: VIEWPORTS[0] },
];

// The thread of the long row open as drawn, axe-clean and matching its screenshot.
async function drawnOpen(page: Page, drawn: Drawn, theme: (typeof THEMES)[number]): Promise<void> {
  await test.step(`${drawn.name} in ${theme} at ${drawn.viewport.name} width`, async () => {
    await page.emulateMedia({ colorScheme: theme });
    await openInbox(page, drawn.readingPane);
    await rowAt(page, LONG_ROW).click();
    await expect(page.getByRole("heading", { name: subjectOf(LONG_ROW) })).toBeVisible();
    await settled(page);
    const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
    expect.soft(results.violations, `axe on ${drawn.name} in ${theme}`).toEqual([]);
    await expect
      .soft(page)
      .toHaveScreenshot(`thread-${drawn.name}-${theme}-${drawn.viewport.name}.png`);
  });
}

test("the open thread is axe-clean and matches its screenshots in every position", async ({
  page,
}) => {
  for (const drawn of DRAWN) {
    await page.setViewportSize({ width: drawn.viewport.width, height: drawn.viewport.height });
    for (const theme of THEMES) {
      await drawnOpen(page, drawn, theme);
    }
  }
});

test.describe("on a touchscreen", () => {
  test.use({ hasTouch: true });

  test("below the list at the tablet width the seam keeps its band and grip and the rows their touch height", async ({
    page,
  }) => {
    const drawn: Drawn = { name: "bottom", readingPane: "bottom", viewport: TABLET };
    await page.setViewportSize({ width: TABLET.width, height: TABLET.height });
    for (const theme of THEMES) {
      await drawnOpen(page, drawn, theme);
    }
    expect(await page.evaluate(() => matchMedia("(any-pointer: coarse)").matches)).toBe(true);
    const seam = page.getByRole("separator", { name: "Resize the list" });
    expect(await heightOf(seam)).toBe(HIT_TARGET_PX);
    expect(
      await seam.evaluate((element) => ({
        band: getComputedStyle(element, "::before").blockSize,
        grip: getComputedStyle(element, "::after").inlineSize,
      })),
    ).toEqual({ band: `${String(TOUCH_BAND_PX)}px`, grip: `${String(GRIP_WIDTH_PX)}px` });
    expect(await heightOf(page.getByRole("main"))).toBe(
      TOUCH_TOOLBAR_PX + TOUCH_FOOT_PX + LIST_DEFAULT_ROWS * TOUCH_ROW_PX,
    );
    expect(await heightOf(grid(page))).toBeGreaterThanOrEqual(LIST_DEFAULT_ROWS * TOUCH_ROW_PX);
    // The band takes its 12 px from the list's room, never from the pane's least height.
    await seam.focus();
    await page.keyboard.press("End");
    expect(await heightOf(pane(page))).toBeGreaterThanOrEqual(PANE_MIN_HEIGHT_PX);
  });
});
