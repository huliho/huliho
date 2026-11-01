// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The folders the user makes, renames, hides and deletes from the tree,
// the pane header and the palette, against a server that lets them.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import type { MailServer } from "./mail-answers";
import { FOLDER_MAILBOXES } from "./mail-mocks";
import type { MailOptions } from "./mail-mocks";
import { THEMES, VIEWPORTS, axeOn } from "./sweep";
import { INBOX_PATH, openInbox, settled } from "./thread-pane";

const FACTUREN_PATH = "/mail/acc-1/mb-facturen";
const OFFERTES_PATH = "/mail/acc-1/mb-offertes";
// The messages Facturen holds in the mocked tree.
const FACTUREN_TOTAL = 40;
const MOD = "ControlOrMeta";
const FOLDER_COMMANDS = [
  "New folder",
  "New subfolder",
  "Rename folder",
  "Hide folder",
  "Delete folder",
];

function header(page: Page) {
  return page.locator("main header");
}

function toast(page: Page, text: string) {
  return page.getByRole("dialog").filter({ hasText: text });
}

function newFolderButton(page: Page) {
  return page.getByRole("button", { name: "New folder", exact: true });
}

// The inbox, or the mailbox at `path`, on a server that lets the user
// manage the folders.
async function managing(page: Page, path = INBOX_PATH, mail: MailOptions = {}) {
  return openInbox(page, "right", path, { mailboxes: FOLDER_MAILBOXES, mail });
}

function folderNamed(server: MailServer, name: string): Record<string, unknown> | undefined {
  return server.mailboxes.find((row) => row["name"] === name);
}

test("New folder at the caption makes a folder at the top; the focus comes back to the button", async ({
  page,
}) => {
  const { server } = await managing(page);
  await newFolderButton(page).click();
  const dialog = page.getByRole("dialog", { name: "New folder" });
  await expect(dialog).toContainText("At the top level");
  const field = dialog.getByRole("textbox", { name: "Folder name" });
  await expect(field).toBeFocused();
  expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
  await settled(page);
  for (const theme of THEMES) {
    await page.emulateMedia({ colorScheme: theme });
    await expect.soft(dialog).toHaveScreenshot(`folder-dialog-${theme}-desktop.png`);
  }
  await field.fill("Facturen");
  await dialog.getByRole("button", { name: "Create" }).click();
  await expect(dialog).toContainText("A folder with that name is already there.");
  await field.fill("Projecten");
  await dialog.getByRole("button", { name: "Create" }).click();
  await expect(toast(page, "Folder created.")).toBeVisible();
  await expect(dialog).toBeHidden();
  await expect(page.getByRole("treeitem", { name: "Projecten" })).toBeVisible();
  await expect(newFolderButton(page)).toBeFocused();
  expect(folderNamed(server, "Projecten")).toMatchObject({ parentId: null, isSubscribed: true });
});

test("a subfolder from the tree's menu hands the focus back to the item; a rename from the header's keeps the route", async ({
  page,
}) => {
  await managing(page);
  const facturen = page.getByRole("treeitem", { name: "Facturen, 3 unread" });
  await facturen.click({ button: "right" });
  await page.getByRole("menuitem", { name: "New subfolder" }).click();
  const create = page.getByRole("dialog", { name: "New folder" });
  await expect(create).toContainText("In Facturen");
  await create.getByRole("textbox", { name: "Folder name" }).fill("2026");
  await create.getByRole("button", { name: "Create" }).click();
  await expect(page.getByRole("treeitem", { name: "2026" })).toHaveAttribute("aria-level", "2");
  await expect(facturen).toBeFocused();
  await facturen.click();
  await header(page).getByRole("button", { name: "Facturen", exact: true }).click();
  await page.getByRole("menuitem", { name: "Rename" }).click();
  const rename = page.getByRole("dialog", { name: "Rename folder" });
  const field = rename.getByRole("textbox", { name: "Folder name" });
  await expect(field).toHaveValue("Facturen");
  await expect(field).toBeFocused();
  expect(
    await field.evaluate((input: HTMLInputElement) => [input.selectionStart, input.selectionEnd]),
  ).toEqual([0, "Facturen".length]);
  await field.fill("Facturen 2026");
  await rename.getByRole("button", { name: "Rename" }).click();
  await expect(toast(page, "Folder renamed.")).toBeVisible();
  await expect(page).toHaveURL(FACTUREN_PATH);
  await expect(header(page).getByRole("heading", { level: 1 })).toHaveText("Facturen 2026");
});

test("Hide takes a folder out of the tree with its subfolders; a subfolder shown leaves for the inbox", async ({
  page,
}) => {
  const { server } = await managing(page, OFFERTES_PATH);
  await page.getByRole("treeitem", { name: "Verbouwing" }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Hide" }).click();
  await expect(page).toHaveURL(INBOX_PATH);
  await expect(toast(page, "Folder hidden.")).toBeVisible();
  await expect(page.getByRole("treeitem", { name: "Verbouwing" })).toHaveCount(0);
  await expect(page.getByRole("treeitem", { name: "Offertes" })).toHaveCount(0);
  expect(folderNamed(server, "Offertes")).toMatchObject({ isSubscribed: false });
});

test("Delete refuses a folder with subfolders and asks once for one with mail, Cancel first", async ({
  page,
}) => {
  const { server } = await managing(page, FACTUREN_PATH);
  await page.getByRole("treeitem", { name: "Verbouwing" }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Delete" }).click();
  await expect(toast(page, "Delete its subfolders first.")).toBeVisible();
  await header(page).getByRole("button", { name: "Facturen", exact: true }).click();
  await page.getByRole("menuitem", { name: "Delete" }).click();
  const dialog = page.getByRole("dialog", { name: "Delete “Facturen”?" });
  await expect(dialog).toContainText(`Its ${String(FACTUREN_TOTAL)} messages will be deleted`);
  await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();
  expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
  await settled(page);
  for (const theme of THEMES) {
    await page.emulateMedia({ colorScheme: theme });
    await expect.soft(dialog).toHaveScreenshot(`delete-dialog-${theme}-desktop.png`);
  }
  await page.keyboard.press("Escape");
  await expect(header(page).getByRole("button", { name: "Facturen", exact: true })).toBeFocused();
  expect(folderNamed(server, "Facturen")).toBeDefined();
  await page.keyboard.press("Enter");
  await page.getByRole("menuitem", { name: "Delete" }).click();
  await dialog.getByRole("button", { name: "Delete folder" }).click();
  await expect(page).toHaveURL(INBOX_PATH);
  await expect(toast(page, "Folder deleted.")).toBeVisible();
  await expect(page.getByRole("treeitem", { name: /^Facturen/ })).toHaveCount(0);
  await expect(header(page).getByRole("button", { name: "Inbox", exact: true })).toBeFocused();
  expect(folderNamed(server, "Facturen")).toBeUndefined();
});

test("a confirmed delete from the tree's menu hands the focus to the tree's tab stop, by pointer and by keyboard", async ({
  page,
}) => {
  await managing(page);
  const inbox = page.getByRole("treeitem", { name: "Inbox, 23 unread" });
  await page.getByRole("treeitem", { name: "Facturen, 3 unread" }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Delete" }).click();
  const facturen = page.getByRole("dialog", { name: "Delete “Facturen”?" });
  await facturen.getByRole("button", { name: "Delete folder" }).click();
  await expect(toast(page, "Folder deleted.")).toBeVisible();
  await expect(page.getByRole("treeitem", { name: /^Facturen/ })).toHaveCount(0);
  await expect(inbox).toBeFocused();
  await page.getByRole("treeitem", { name: "Offertes" }).focus();
  await page.keyboard.press("Shift+F10");
  await expect(page.getByRole("menu")).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("End");
  await expect(page.getByRole("menuitem", { name: "Delete" })).toBeFocused();
  await page.keyboard.press("Enter");
  const offertes = page.getByRole("dialog", { name: "Delete “Offertes”?" });
  await expect(offertes.getByRole("button", { name: "Cancel" })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(offertes.getByRole("button", { name: "Delete folder" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("treeitem", { name: "Offertes" })).toHaveCount(0);
  await expect(inbox).toBeFocused();
});

test("the server's refusal reads in the dialog's failure line and the field keeps its text", async ({
  page,
}) => {
  const { server } = await managing(page);
  server.folderRefusal = "forbidden";
  await newFolderButton(page).click();
  const dialog = page.getByRole("dialog", { name: "New folder" });
  const field = dialog.getByRole("textbox", { name: "Folder name" });
  await field.fill("Projecten");
  await dialog.getByRole("button", { name: "Create" }).click();
  await expect(dialog.getByRole("alert")).toHaveText("The server refused. Try again.");
  await expect(field).toHaveValue("Projecten");
  server.folderRefusal = null;
  await dialog.getByRole("button", { name: "Create" }).click();
  await expect(dialog).toBeHidden();
  await expect(page.getByRole("treeitem", { name: "Projecten" })).toBeVisible();
});

test("the folder commands stand in the palette without a key and open their dialogs", async ({
  page,
}) => {
  await managing(page, FACTUREN_PATH);
  await page.getByRole("treeitem", { name: "Facturen, 3 unread" }).focus();
  await page.keyboard.press(`${MOD}+k`);
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("folder");
  for (const label of FOLDER_COMMANDS) {
    await expect(palette.getByRole("option", { name: label, exact: true })).toHaveCount(1);
  }
  await palette.getByRole("option", { name: "Rename folder", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "Rename folder" })).toBeVisible();
});

test("the name dialog reads right to left in the pseudo-locale", async ({ page }) => {
  await managing(page);
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "en-XA");
  });
  await page.goto(INBOX_PATH);
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
  await page.getByRole("button", { name: /Ñéw fóldér/ }).click();
  const dialog = page.getByRole("dialog", { name: /Ñéw fóldér/ });
  await expect(dialog).toBeVisible();
  await settled(page);
  await expect.soft(dialog).toHaveScreenshot("folder-dialog-en-XA-light-desktop.png");
});

test.describe("on a phone", () => {
  test.use({ viewport: VIEWPORTS[0], hasTouch: true, isMobile: true });

  test("New folder in the sidebar's sheet opens the dialog across the screen", async ({ page }) => {
    await managing(page);
    await header(page).getByRole("button", { name: "Mailboxes and accounts" }).click();
    const button = newFolderButton(page);
    // The sheet slides in; a trial click waits until the button stands still.
    await button.click({ trial: true });
    await button.click();
    const dialog = page.getByRole("dialog", { name: "New folder" });
    await expect(dialog.getByRole("textbox", { name: "Folder name" })).toBeFocused();
    expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
    await settled(page);
    await expect.soft(page).toHaveScreenshot("folder-dialog-light-phone.png");
  });
});
