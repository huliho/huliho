// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { AxeBuilder } from "@axe-core/playwright";
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { accountRow, mockAccounts } from "./account-mocks";
import { FIXED_NOW } from "./mail-corpus";
import { MAILBOXES, mockMail } from "./mail-mocks";
import type { MailboxBody } from "./mail-mocks";
import { mockPreferences } from "./preference-mocks";
import { mockSignedIn } from "./session-mocks";
import { THEMES, VIEWPORTS, WCAG_TAGS } from "./sweep";
import { settled } from "./thread-pane";

const HOUR_MS = 3_600_000;
const ROWS = [
  accountRow(FIXED_NOW),
  accountRow(FIXED_NOW, {
    id: "acc-2",
    address: "s.bakker@gmail.com",
    name: "Gmail",
    provider: "gmail",
    kind: "imap",
    authMethod: "password",
    createdAt: FIXED_NOW.getTime() - HOUR_MS / 2,
  }),
];
// The platform's command modifier, as Playwright names it.
const MOD = "ControlOrMeta";
const PSEUDO_LOCALE = "en-XA";
// A phone shorter than the overlay over a tree with the folders below, so the popup has to scroll.
const SHORT_VIEWPORT = { width: 360, height: 640 };
// Folders whose first letter is free in the fixture tree, so each adds a keyed row under Go.
const TALL_TREE_FOLDERS = ["Berlin", "Cairo", "Eindhoven", "Geneve", "Hanoi", "Kyoto"];
// Sorted after the fixture's own folders.
const EXTRA_FOLDER_ORDER = 20;

interface InboxOptions {
  locale?: string;
  mailboxes?: MailboxBody[];
}

// The fixture tree with a flat folder per name, one message in each.
function withFolders(names: readonly string[]): MailboxBody[] {
  const base = MAILBOXES.find((row) => row.role === null);
  if (base === undefined) {
    throw new Error("the fixture tree has no folder");
  }
  const extra = names.map((name, index) => ({
    ...base,
    id: `mb-${name.toLowerCase()}`,
    name,
    parentId: null,
    sortOrder: EXTRA_FOLDER_ORDER + index,
    totalEmails: 1,
    unreadEmails: 0,
    totalThreads: 1,
    unreadThreads: 0,
  }));
  return [...MAILBOXES, ...extra];
}

async function openInbox(page: Page, options: InboxOptions = {}): Promise<void> {
  await mockSignedIn(page);
  await mockMail(page, options.mailboxes);
  await mockAccounts(page, ROWS);
  if (options.locale !== undefined) {
    await mockPreferences(page, { locale: options.locale });
  }
  await page.clock.setFixedTime(FIXED_NOW);
  await page.goto("/mail/acc-1/mb-inbox");
}

function rowAt(page: Page, index: number): Locator {
  return page.getByRole("grid").locator(`[aria-rowindex="${String(index)}"]`);
}

function palette(page: Page): Locator {
  return page.getByRole("dialog", { name: "Command palette" });
}

function overlay(page: Page): Locator {
  return page.getByRole("dialog", { name: "Keyboard" });
}

async function boxOf(
  locator: Locator,
): Promise<{ x: number; y: number; width: number; height: number }> {
  const box = await locator.boundingBox();
  if (box === null) {
    throw new Error("the element has no box");
  }
  return box;
}

// Whether the element painted topmost at a point of the viewport belongs to a dialog.
async function dialogAt(page: Page, x: number, y: number): Promise<boolean> {
  return page.evaluate(
    ({ atX, atY }) => {
      const hit = document.elementFromPoint(atX, atY);
      return hit !== null && hit.closest('[role="dialog"]') !== null;
    },
    { atX: x, atY: y },
  );
}

// The seam between the list and the reading pane crosses the popup; the popup paints over it.
async function expectOverSeam(page: Page, popup: Locator, seamX: number): Promise<void> {
  await settled(page);
  const box = await boxOf(popup);
  expect(seamX).toBeGreaterThan(box.x);
  expect(seamX).toBeLessThan(box.x + box.width);
  expect(await dialogAt(page, seamX, box.y + box.height / 2)).toBe(true);
}

test("the question mark opens the overlay, which lists what the palette lists", async ({
  page,
}) => {
  await openInbox(page);
  await rowAt(page, 1).focus();
  await page.keyboard.press("?");
  await expect(overlay(page)).toBeVisible();
  const listed = await overlay(page).locator("dt").allTextContents();
  expect(listed).toContain("Go to Inbox");
  expect(listed).toContain("Next conversation");
  expect(listed).toContain("Switch account");
  expect(listed).toContain("Command palette");
  await expect(overlay(page).getByText("Go to Inbox").locator("xpath=..")).toContainText("g i");
  await page.keyboard.press("Escape");
  await expect(overlay(page)).toBeHidden();
  await expect(rowAt(page, 1)).toBeFocused();
  await page.keyboard.press(`${MOD}+k`);
  await expect(palette(page)).toBeVisible();
  for (const label of listed) {
    await expect(palette(page).getByRole("option", { name: label, exact: true })).toHaveCount(1);
  }
  expect(await palette(page).getByRole("option").count()).toBe(listed.length);
});

test("the palette narrows on a query and Enter runs the jump with the focus on the first row", async ({
  page,
}) => {
  await openInbox(page);
  await rowAt(page, 1).focus();
  await page.keyboard.press(`${MOD}+k`);
  const input = palette(page).getByRole("combobox", { name: "Command palette" });
  await expect(input).toBeFocused();
  await page.keyboard.type("dra");
  await expect(palette(page).getByRole("option")).toHaveText(["Go to Draftsg d"]);
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-drafts$/);
  await expect(palette(page)).toBeHidden();
  await expect(page.getByRole("heading", { level: 1, name: "Drafts" })).toBeVisible();
  await expect(rowAt(page, 1)).toBeFocused();
  // The command last run leads the list the next time the palette opens.
  await page.keyboard.press(`${MOD}+k`);
  await expect(palette(page).getByRole("option").first()).toHaveText("Go to Draftsg d");
  await expect(palette(page).getByText("Recent")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(rowAt(page, 1)).toBeFocused();
});

test("the no-match region stands in the palette before use and gets its sentence", async ({
  page,
}) => {
  await openInbox(page);
  await rowAt(page, 1).focus();
  await page.keyboard.press(`${MOD}+k`);
  await expect(palette(page).getByRole("option").first()).toBeVisible();
  // The region is in the accessibility tree while the list has matches, holding nothing.
  const region = palette(page).getByRole("status");
  await expect(region).toHaveCount(1);
  await expect(region).toBeEmpty();
  expect(await region.evaluate((element) => getComputedStyle(element).display)).not.toBe("none");
  expect(await region.evaluate((element) => element.getBoundingClientRect().height)).toBe(0);
  await page.keyboard.type("zzz");
  await expect(palette(page).getByRole("option")).toHaveCount(0);
  await expect(region).toHaveText("No command matches.");
});

test("g then a letter jumps, a letter that completes nothing does nothing and keys never fire while typing", async ({
  page,
}) => {
  await openInbox(page);
  await rowAt(page, 1).focus();
  await page.keyboard.press("g");
  await page.keyboard.press("t");
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-trash$/);
  await expect(rowAt(page, 1)).toBeFocused();
  await page.keyboard.press("g");
  await page.keyboard.press("x");
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-trash$/);
  await page.keyboard.press("j");
  await expect(rowAt(page, 2)).toBeFocused();
  await page.keyboard.press(`${MOD}+k`);
  await expect(palette(page).getByRole("combobox")).toBeFocused();
  await page.keyboard.type("jk?");
  await expect(palette(page).getByRole("combobox")).toHaveValue("jk?");
  await expect(overlay(page)).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(rowAt(page, 2)).toBeFocused();
});

test("the switch command opens the account menu with the keyboard inside it", async ({ page }) => {
  await openInbox(page);
  // The landed trigger carries an id; the key presses it as a keyboard would.
  await expect(page.locator('nav button[aria-haspopup="menu"][id]')).toBeVisible();
  await rowAt(page, 1).focus();
  await page.keyboard.press(`${MOD}+Shift+l`);
  const menu = page.getByRole("menu");
  await expect(menu).toBeVisible();
  await expect(menu.getByRole("menuitemradio")).toHaveCount(ROWS.length);
  await expect(menu.getByRole("menuitemradio").first()).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(menu.getByRole("menuitemradio").nth(1)).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  await expect(page.getByRole("button", { name: /sanne@fastmail\.com/ })).toBeFocused();
});

test("the palette and the overlay stand over the seam between the panes", async ({ page }) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await openInbox(page);
  await rowAt(page, 1).focus();
  const seam = await boxOf(page.getByRole("separator", { name: "Resize the list" }));
  const seamX = seam.x + seam.width / 2;
  await page.keyboard.press(`${MOD}+k`);
  await expect(palette(page)).toBeVisible();
  await expectOverSeam(page, palette(page), seamX);
  await page.keyboard.press("Escape");
  await expect(palette(page)).toBeHidden();
  await page.keyboard.press("?");
  await expect(overlay(page)).toBeVisible();
  await expectOverSeam(page, overlay(page), seamX);
});

test("the key hints stand under the list where a keyboard is likely", async ({ page }) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await openInbox(page);
  await expect(rowAt(page, 1)).toBeVisible();
  const main = page.getByRole("main");
  await expect(main.getByText("j/k")).toBeVisible();
  await expect(main.getByText("shortcuts")).toBeVisible();
  await expect(main.getByText("undo")).toHaveCount(0);
  await page.setViewportSize(VIEWPORTS[0]);
  await expect(main.getByText("j/k")).toHaveCount(0);
});

test("the palette and the overlay are axe-clean and match their screenshots at two widths", async ({
  page,
}) => {
  for (const viewport of VIEWPORTS) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    for (const theme of THEMES) {
      await test.step(`${theme} at ${viewport.name} width`, async () => {
        await page.emulateMedia({ colorScheme: theme });
        await openInbox(page);
        await rowAt(page, 1).focus();
        await page.keyboard.press(`${MOD}+k`);
        await expect(palette(page)).toBeVisible();
        await settled(page);
        const onPalette = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
        expect.soft(onPalette.violations, `axe on the palette in ${theme}`).toEqual([]);
        await expect.soft(page).toHaveScreenshot(`palette-${theme}-${viewport.name}.png`);
        await page.keyboard.press("Escape");
        await expect(palette(page)).toBeHidden();
        await page.keyboard.press("?");
        await expect(overlay(page)).toBeVisible();
        await settled(page);
        const onOverlay = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
        expect.soft(onOverlay.violations, `axe on the overlay in ${theme}`).toEqual([]);
        await expect.soft(page).toHaveScreenshot(`shortcuts-${theme}-${viewport.name}.png`);
      });
    }
  }
});

test("the overlay stays inside a short viewport and its last row scrolls into view by key", async ({
  page,
}) => {
  await page.setViewportSize(SHORT_VIEWPORT);
  await openInbox(page, { mailboxes: withFolders(TALL_TREE_FOLDERS) });
  await rowAt(page, 1).focus();
  await page.keyboard.press("?");
  await expect(overlay(page)).toBeVisible();
  await expect(overlay(page)).toBeFocused();
  await settled(page);
  const box = await boxOf(overlay(page));
  expect(box.y).toBeGreaterThanOrEqual(0);
  expect(box.y + box.height).toBeLessThanOrEqual(SHORT_VIEWPORT.height);
  const title = overlay(page).getByRole("heading", { name: "Keyboard" });
  const lastRow = overlay(page).locator("dt").last();
  await expect(lastRow).toHaveText("Keyboard shortcuts");
  await expect(title).toBeInViewport();
  await expect(lastRow).not.toBeInViewport();
  await page.keyboard.press("End");
  await expect(lastRow).toBeInViewport();
  await page.keyboard.press("Home");
  await expect(title).toBeInViewport();
});

test("the palette and the overlay read in Dutch and in the pseudo-locale", async ({ page }) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await openInbox(page, { locale: "nl" });
  await page.getByRole("grid", { name: "Gesprekken" }).locator('[aria-rowindex="1"]').focus();
  await page.keyboard.press(`${MOD}+k`);
  const dutch = page.getByRole("dialog", { name: "Opdrachtenpalet" });
  await expect(dutch.getByRole("combobox")).toHaveAttribute(
    "placeholder",
    "Typ een opdracht of zoek…",
  );
  await expect(dutch.getByRole("option", { name: "Ga naar Inbox", exact: true })).toHaveCount(1);
  await settled(page);
  await expect.soft(page).toHaveScreenshot("palette-nl-light-desktop.png");
  await page.keyboard.press("Escape");
  await expect(dutch).toBeHidden();
  await page.keyboard.press("?");
  await expect(page.getByRole("dialog", { name: "Toetsenbord" })).toBeVisible();
  await expect(page.getByText("Van account wisselen")).toBeVisible();
  await page.keyboard.press("Escape");
  // The pseudo-locale lives on the device alone and never reaches the server.
  await page.addInitScript((locale) => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", locale);
  }, PSEUDO_LOCALE);
  await page.goto("/mail/acc-1/mb-inbox");
  await page.getByRole("grid").locator('[aria-rowindex="1"]').focus();
  await page.keyboard.press(`${MOD}+k`);
  const mirrored = page.getByRole("dialog");
  await expect(mirrored).toBeVisible();
  expect(await mirrored.evaluate((element) => getComputedStyle(element).direction)).toBe("rtl");
  await settled(page);
  await expect.soft(page).toHaveScreenshot(`palette-${PSEUDO_LOCALE}-light-desktop.png`);
  await page.keyboard.press("Escape");
  await expect(mirrored).toBeHidden();
  await page.keyboard.press("?");
  await expect(page.getByRole("dialog")).toBeVisible();
  await settled(page);
  await expect.soft(page).toHaveScreenshot(`shortcuts-${PSEUDO_LOCALE}-light-desktop.png`);
});
