// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { AxeBuilder } from "@axe-core/playwright";
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { accountRow, mockAccounts } from "./account-mocks";
import type { AccountRowBody } from "./account-mocks";
import { FIXED_NOW } from "./mail-corpus";
import { MAILBOXES, mockMail, refuseMailWhile } from "./mail-mocks";
import type { MailboxBody } from "./mail-mocks";
import { mockPreferences } from "./preference-mocks";
import { mockSignedIn } from "./session-mocks";
import { THEMES, VIEWPORTS, WCAG_TAGS } from "./sweep";

const HOUR_MS = 3_600_000;
// The shell has two widths of its own beside the sweep's: the smallest
// viewport the product supports and the tablet reference.
const NARROW = { name: "narrow", width: 320, height: 568 };
const TABLET = { name: "tablet", width: 834, height: 1112 };
// The narrowest window that gets the tablet layout.
const TABLET_FLOOR = { name: "tablet floor", width: 720, height: 1112 };
const SHELL_VIEWPORTS = [NARROW, VIEWPORTS[0], TABLET, VIEWPORTS[1]];
// The design's list width and one arrow key's step on the seam.
const LIST_WIDTH_PX = 360;
const STEP_PX = 16;
const DRAG_PX = 100;
const SIDEBAR_WIDTH_PX = 240;
const PANE_MIN_PX = 320;
// A stored list width wider than any window here, and the touch target size.
const STORED_LIST_WIDTH_PX = 1360;
const HIT_TARGET_PX = 44;
// A point on the scrim beside the sheet on a phone.
const SCRIM_X = 380;
const SCRIM_Y = 300;

const OLDEST = accountRow(FIXED_NOW);
const NEWER = accountRow(FIXED_NOW, {
  id: "acc-2",
  address: "s.bakker@gmail.com",
  name: "Gmail",
  provider: "gmail",
  kind: "imap",
  authMethod: "password",
  createdAt: FIXED_NOW.getTime() - HOUR_MS / 2,
});
const ROWS = [OLDEST, NEWER];
const EMPTY_INBOX: MailboxBody[] = MAILBOXES.map((mailbox) =>
  mailbox.role === "inbox" ? { ...mailbox, totalEmails: 0, unreadEmails: 0 } : mailbox,
);

async function openShell(page: Page, path = "/", rows: AccountRowBody[] = ROWS): Promise<void> {
  await mockSignedIn(page);
  await mockAccounts(page, rows);
  await page.goto(path);
  await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
}

function tree(page: Page): Locator {
  return page.getByRole("tree", { name: "Mailboxes" });
}

function switcher(page: Page): Locator {
  return page.getByRole("button", { name: /sanne@fastmail\.com/ });
}

async function widthOf(locator: Locator): Promise<number> {
  const box = await locator.boundingBox();
  return box?.width ?? 0;
}

// The rows as a screen reader names them, in drawing order.
function names(locator: Locator): Promise<(string | null)[]> {
  return locator.evaluateAll((items) => items.map((item) => item.getAttribute("aria-label")));
}

// The reading pane keeps its minimum, the list is narrower than the
// design's default and the seam announces the list's width.
async function expectListClamped(page: Page): Promise<void> {
  const main = page.getByRole("main");
  const reading = page.getByRole("complementary", { name: "Conversation" });
  expect(await widthOf(reading)).toBe(PANE_MIN_PX);
  expect(await widthOf(main)).toBeLessThan(LIST_WIDTH_PX);
  await expect(page.getByRole("separator", { name: "Resize the list" })).toHaveAttribute(
    "aria-valuenow",
    String(await widthOf(main)),
  );
}

test("the root lands in the oldest account's inbox and draws its tree", async ({ page }) => {
  await openShell(page);
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-inbox$/);
  await expect(page.getByRole("heading", { level: 1, name: "Inbox" })).toBeVisible();
  await expect(page.getByRole("main")).toContainText("23");
  await expect(page.getByRole("main").getByText("23 unread")).toBeAttached();
  await expect(tree(page).getByRole("treeitem")).toHaveCount(MAILBOXES.length);
  expect(await names(tree(page).getByRole("treeitem"))).toEqual([
    "Inbox, 23 unread",
    "Drafts, 2 drafts",
    "Sent",
    "Archive",
    "Junk, 1 unread",
    "Trash",
    "Facturen, 3 unread",
    "Verbouwing",
    "Offertes",
  ]);
  const inbox = tree(page).getByRole("treeitem", { name: "Inbox, 23 unread" });
  await expect(inbox).toHaveAttribute("aria-current", "page");
  await expect(tree(page).getByRole("treeitem", { name: "Drafts" })).toContainText("2");
  await expect(tree(page).getByRole("treeitem", { name: "Offertes" })).toHaveAttribute(
    "aria-level",
    "2",
  );
  const verbouwing = tree(page).getByRole("treeitem", { name: "Verbouwing" });
  await expect(verbouwing.locator('[aria-hidden="true"]')).toHaveText("v");
  await expect(page.getByText("Folders")).toBeVisible();
  await expect(page.getByRole("complementary", { name: "Conversation" })).toContainText(
    "Select a conversation.",
  );
});

test("a mailbox picked in the tree opens with its own header", async ({ page }) => {
  await openShell(page);
  await tree(page).getByRole("treeitem", { name: "Facturen, 3 unread" }).click();
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-facturen$/);
  await expect(page.getByRole("heading", { level: 1, name: "Facturen" })).toBeVisible();
  await expect(page.getByRole("main")).toContainText("3");
  await expect(tree(page).getByRole("treeitem", { name: "Facturen, 3 unread" })).toHaveAttribute(
    "aria-current",
    "page",
  );
  await expect(tree(page).getByRole("treeitem", { name: "Inbox, 23 unread" })).not.toHaveAttribute(
    "aria-current",
    "page",
  );
});

test("an empty mailbox says so and points at the inbox", async ({ page }) => {
  await openShell(page, "/mail/acc-1/mb-verbouwing");
  await expect(page.getByText("Verbouwing is empty.")).toBeVisible();
  await page.getByRole("link", { name: "Open Inbox" }).click();
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-inbox$/);
});

test("an empty inbox points at the archive", async ({ page }) => {
  await mockSignedIn(page);
  await mockMail(page, EMPTY_INBOX);
  await mockAccounts(page, ROWS);
  await page.goto("/");
  await expect(page.getByText("Inbox is empty. Nothing needs you right now.")).toBeVisible();
  await page.getByRole("link", { name: "Open Archive" }).click();
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-archive$/);
});

test("the arrow keys walk the tree and Tab leaves it as one stop", async ({ page }) => {
  await openShell(page);
  await tree(page).getByRole("treeitem", { name: "Inbox, 23 unread" }).focus();
  await page.keyboard.press("ArrowDown");
  await expect(tree(page).getByRole("treeitem", { name: "Drafts" })).toBeFocused();
  await page.keyboard.press("End");
  await expect(tree(page).getByRole("treeitem", { name: "Offertes" })).toBeFocused();
  await page.keyboard.press("Home");
  await expect(tree(page).getByRole("treeitem", { name: "Inbox, 23 unread" })).toBeFocused();
  // Tab reaches the list once its first page is in.
  await expect(page.getByRole("grid").locator('[aria-rowindex="1"]')).toBeVisible();
  await page.keyboard.press("Tab");
  await expect(page.getByRole("grid").locator('[aria-rowindex="1"]')).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(page.getByRole("separator", { name: "Resize the list" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-inbox$/);
});

test("the account menu lists every account, switches and reaches settings", async ({ page }) => {
  await openShell(page);
  await switcher(page).click();
  const menu = page.getByRole("menu");
  await expect(menu.getByRole("menuitemradio")).toHaveText([
    /Fastmail.*sanne@fastmail\.com/,
    /Gmail.*s\.bakker@gmail\.com/,
  ]);
  await expect(menu.getByRole("menuitemradio", { name: /Fastmail/ })).toHaveAttribute(
    "aria-checked",
    "true",
  );
  await menu.getByRole("menuitemradio", { name: /Gmail/ }).click();
  await expect(page).toHaveURL(/\/mail\/acc-2\/mb-inbox$/);
  await expect(menu).toHaveCount(0);
  await page.getByRole("button", { name: /s\.bakker@gmail\.com/ }).click();
  await page.getByRole("menuitem", { name: "Settings" }).click();
  await expect(page).toHaveURL(/\/settings\/accounts$/);
});

test("Sign out in the account menu ends the session and forgets the account", async ({ page }) => {
  await openShell(page);
  await switcher(page).click();
  await page.getByRole("menuitem", { name: "Sign out" }).click();
  await expect(page).toHaveURL(/\/sign-in$/);
  expect(await page.evaluate(() => localStorage.getItem("huliho-last-account"))).toBeNull();
});

test("the root returns to the account this device opened last", async ({ page }) => {
  await openShell(page, "/mail/acc-2");
  await expect(page).toHaveURL(/\/mail\/acc-2\/mb-inbox$/);
  await page.goto("/");
  await expect(page).toHaveURL(/\/mail\/acc-2\/mb-inbox$/);
  await page.goto("/mail/acc-9");
  await expect(page).toHaveURL(/\/mail\/acc-2\/mb-inbox$/);
  await page.goto("/mail/acc-1/mb-nowhere");
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-inbox$/);
});

test("a tree that fails to load says so and Try again brings it", async ({ page }) => {
  let failing = true;
  await mockSignedIn(page);
  await mockAccounts(page, ROWS);
  await refuseMailWhile(page, () => failing);
  await page.goto("/");
  await expect(page.getByRole("alert")).toContainText("Couldn’t load your mail.");
  await expect(tree(page)).toHaveCount(0);
  failing = false;
  await page.getByRole("button", { name: "Try again" }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Inbox" })).toBeVisible();
  await expect(tree(page).getByRole("treeitem")).toHaveCount(MAILBOXES.length);
});

test("the seam moves by pointer and keyboard, stays across a reload and resets", async ({
  page,
}) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await openShell(page);
  const main = page.getByRole("main");
  const seam = page.getByRole("separator", { name: "Resize the list" });
  expect(await widthOf(main)).toBe(LIST_WIDTH_PX);
  await expect(seam).toHaveAttribute("aria-valuemin", String(PANE_MIN_PX));
  await expect(seam).toHaveAttribute(
    "aria-valuemax",
    String(VIEWPORTS[1].width - SIDEBAR_WIDTH_PX - PANE_MIN_PX),
  );
  const listId = await main.getAttribute("id");
  expect(listId).not.toBeNull();
  await expect(seam).toHaveAttribute("aria-controls", String(listId));
  const box = await seam.boundingBox();
  if (box === null) {
    throw new Error("the seam has no box");
  }
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + DRAG_PX, y, { steps: 4 });
  await page.mouse.up();
  expect(await widthOf(main)).toBe(LIST_WIDTH_PX + DRAG_PX);
  await page.reload();
  await expect(page.getByRole("heading", { level: 1, name: "Inbox" })).toBeVisible();
  expect(await widthOf(main)).toBe(LIST_WIDTH_PX + DRAG_PX);
  await seam.focus();
  await page.keyboard.press("ArrowLeft");
  expect(await widthOf(main)).toBe(LIST_WIDTH_PX + DRAG_PX - STEP_PX);
  await page.keyboard.press("End");
  expect(await widthOf(main)).toBe(VIEWPORTS[1].width - SIDEBAR_WIDTH_PX - PANE_MIN_PX);
  await page.keyboard.press("Home");
  expect(await widthOf(main)).toBe(PANE_MIN_PX);
  await page.keyboard.press("Enter");
  expect(await widthOf(main)).toBe(LIST_WIDTH_PX);
  await seam.dblclick();
  expect(await widthOf(main)).toBe(LIST_WIDTH_PX);
});

test("a width stored on a wider window is clamped so the reading pane keeps its minimum", async ({
  page,
}) => {
  await page.addInitScript((width: number) => {
    window.localStorage.setItem("huliho-list-width", String(width));
  }, STORED_LIST_WIDTH_PX);
  await page.setViewportSize(VIEWPORTS[1]);
  await openShell(page);
  const reading = page.getByRole("complementary", { name: "Conversation" });
  const seam = page.getByRole("separator", { name: "Resize the list" });
  const main = page.getByRole("main");
  const room = VIEWPORTS[1].width - SIDEBAR_WIDTH_PX - PANE_MIN_PX;
  expect(await widthOf(main)).toBe(room);
  await expect(seam).toHaveAttribute("aria-valuenow", String(room));
  expect(await widthOf(reading)).toBe(PANE_MIN_PX);
  await page.setViewportSize(TABLET);
  await expect.poll(() => widthOf(reading)).toBe(PANE_MIN_PX);
  await expect(seam).toHaveAttribute("aria-valuenow", String(await widthOf(main)));
});

test("at the tablet floor the design's default is clamped so the reading pane keeps its minimum", async ({
  page,
}) => {
  await page.setViewportSize(TABLET_FLOOR);
  await openShell(page);
  await expectListClamped(page);
});

test("a phone shows the list alone and the avatar opens the sidebar as a sheet", async ({
  page,
}) => {
  await page.setViewportSize(VIEWPORTS[0]);
  await openShell(page);
  await expect(page.getByRole("navigation")).toHaveCount(0);
  await expect(page.getByRole("complementary")).toHaveCount(0);
  await page.getByRole("button", { name: "Mailboxes and accounts" }).click();
  const sheet = page.getByRole("dialog", { name: "Mailboxes and accounts" });
  await expect(sheet.getByRole("treeitem", { name: "Inbox, 23 unread" })).toBeVisible();
  await sheet.getByRole("treeitem", { name: "Facturen, 3 unread" }).click();
  await expect(sheet).toBeHidden();
  await expect(page.getByRole("heading", { level: 1, name: "Facturen" })).toBeVisible();
  await page.getByRole("button", { name: "Mailboxes and accounts" }).click();
  await expect(sheet).toBeVisible();
  await sheet.getByRole("button", { name: "Close" }).click();
  await expect(sheet).toBeHidden();
  await page.getByRole("button", { name: "Mailboxes and accounts" }).click();
  await expect(sheet).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(sheet).toBeHidden();
  await page.getByRole("button", { name: "Mailboxes and accounts" }).click();
  await expect(sheet).toBeVisible();
  await page.mouse.click(SCRIM_X, SCRIM_Y);
  await expect(sheet).toBeHidden();
  await page.getByRole("button", { name: "Mailboxes and accounts" }).click();
  await sheet.getByRole("button", { name: /sanne@fastmail\.com/ }).click();
  await page.getByRole("menuitemradio", { name: /Gmail/ }).click();
  await expect(sheet).toBeHidden();
  await expect(page).toHaveURL(/\/mail\/acc-2\/mb-inbox$/);
});

test("a tablet shows the rail with the roles and More opens the whole sidebar", async ({
  page,
}) => {
  await page.setViewportSize(TABLET);
  await openShell(page);
  const rail = page.getByRole("navigation", { name: "Mailboxes and accounts" });
  await expect(rail.getByRole("link")).toHaveCount(6);
  await expect(rail.getByRole("link", { name: "Inbox, 23 unread" })).toHaveAttribute(
    "aria-current",
    "page",
  );
  await expect(rail.getByRole("link", { name: /Facturen/ })).toHaveCount(0);
  await rail.getByRole("button", { name: "Account menu" }).click();
  await expect(page.getByRole("menuitemradio")).toHaveCount(2);
  await page.keyboard.press("Escape");
  await rail.getByRole("button", { name: "All mailboxes" }).click();
  const sheet = page.getByRole("dialog", { name: "Mailboxes and accounts" });
  await sheet.getByRole("treeitem", { name: "Facturen, 3 unread" }).click();
  await expect(sheet).toBeHidden();
  await expect(page.getByRole("heading", { level: 1, name: "Facturen" })).toBeVisible();
  await expect(rail.getByRole("link", { name: "Inbox, 23 unread" })).not.toHaveAttribute(
    "aria-current",
    "page",
  );
});

test("the shell is axe-clean and matches its screenshots at four widths", async ({ page }) => {
  for (const viewport of SHELL_VIEWPORTS) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    for (const theme of THEMES) {
      await test.step(`in ${theme} at ${viewport.name} width`, async () => {
        await page.emulateMedia({ colorScheme: theme });
        await openShell(page);
        await expect(page.getByRole("heading", { level: 1, name: "Inbox" })).toBeVisible();
        await page.evaluate(async () => {
          await document.fonts.ready;
        });
        const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
        expect.soft(results.violations, `axe in ${theme} at ${viewport.name} width`).toEqual([]);
        await expect.soft(page).toHaveScreenshot(`mail-${theme}-${viewport.name}.png`);
        await page.goto("/mail/acc-1/mb-verbouwing");
        await expect(page.getByText("Verbouwing is empty.")).toBeVisible();
        await expect.soft(page).toHaveScreenshot(`mail-empty-${theme}-${viewport.name}.png`);
      });
    }
  }
});

test("the shell reads in Dutch and in the pseudo-locale", async ({ page }) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await mockSignedIn(page);
  await mockAccounts(page, ROWS);
  await mockPreferences(page, { locale: "nl" });
  await page.goto("/");
  await expect(page.getByRole("navigation", { name: "Mailboxen en accounts" })).toBeVisible();
  await expect(page.getByText("Mappen")).toBeVisible();
  await expect(page.getByText("Kies een gesprek.")).toBeVisible();
  await expect(
    page.getByRole("tree", { name: "Mailboxen" }).getByRole("treeitem", {
      name: "Inbox, 23 ongelezen",
    }),
  ).toBeVisible();
  await expect.soft(page).toHaveScreenshot("mail-nl-light-desktop.png");
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "en-XA");
  });
  await page.goto("/");
  await expect(page.getByRole("navigation", { name: /Máílbóxés áñd áççóúñts/ })).toBeVisible();
  await expect.soft(page).toHaveScreenshot("mail-en-XA-light-desktop.png");
});

test.describe("on a touchscreen", () => {
  test.use({ hasTouch: true });

  test("the seam and the sheet button meet the touch target size", async ({ page }) => {
    await page.setViewportSize(TABLET);
    await openShell(page);
    expect(await page.evaluate(() => matchMedia("(any-pointer: coarse)").matches)).toBe(true);
    expect(await widthOf(page.getByRole("separator", { name: "Resize the list" }))).toBe(
      HIT_TARGET_PX,
    );
    await page.setViewportSize(TABLET_FLOOR);
    await openShell(page);
    await expectListClamped(page);
    await page.setViewportSize(VIEWPORTS[0]);
    await openShell(page);
    const button = page.getByRole("button", { name: "Mailboxes and accounts" });
    const box = (await button.boundingBox()) ?? { width: 0, height: 0 };
    expect(box.width).toBeGreaterThanOrEqual(HIT_TARGET_PX);
    expect(box.height).toBeGreaterThanOrEqual(HIT_TARGET_PX);
  });
});
