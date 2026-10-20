// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { releaseSets } from "./mail-answers";
import type { MailServer } from "./mail-answers";
import { WRITABLE_MAILBOXES } from "./mail-mocks";
import { THEMES, VIEWPORTS, axeOn } from "./sweep";
import { INBOX_PATH, openInbox, settled, updates } from "./thread-pane";

const TRASH_PATH = "/mail/acc-1/mb-trash";
const JUNK_PATH = "/mail/acc-1/mb-junk";
// The inbox's unread messages and Trash's messages in the mocked tree.
const INBOX_UNREAD = 23;
const TRASH_TOTAL = 12;
// A write takes ten emails, so marking the inbox read takes three rounds.
const ROUND_SIZE = 10;
// A touch rests past the long press before it lifts, a little inside the item.
const LONG_PRESS_HOLD_MS = 700;
const PRESS_INSET_PX = 8;

function header(page: Page) {
  return page.locator("main header");
}

function toast(page: Page, text: string) {
  return page.getByRole("dialog").filter({ hasText: text });
}

// Opens the mailbox's menu from its name and checks the items it holds.
async function menuHolds(page: Page, name: string, items: string[]): Promise<void> {
  await header(page).getByRole("button", { name, exact: true }).click();
  await expect(page.getByRole("menu").getByRole("menuitem")).toHaveText(items);
}

// The inbox's unread messages, each by id, as the corpus seeds them.
function unreadIds(server: MailServer): string[] {
  return [...server.corpus.emails.values()]
    .filter((message) => message.mailboxId === "mb-inbox" && !message.seen)
    .map((message) => message.id);
}

// Every email id each Email/set destroyed.
function destroyed(server: MailServer): string[] {
  return server.sets.flatMap(({ destroy }) =>
    Array.isArray(destroy) ? destroy.filter((id) => typeof id === "string") : [],
  );
}

async function writable(page: Page, path = INBOX_PATH, roundSize?: number) {
  return openInbox(page, "right", path, {
    mailboxes: WRITABLE_MAILBOXES,
    mail: roundSize === undefined ? {} : { maxObjectsInSet: roundSize },
  });
}

test("the mailbox's name opens its menu; Mark all as read marks every unread message", async ({
  page,
}) => {
  const { server } = await writable(page);
  const unread = unreadIds(server);
  expect(unread).toHaveLength(INBOX_UNREAD);
  await menuHolds(page, "Inbox", ["Mark all as read"]);
  await page.getByRole("menuitem", { name: "Mark all as read" }).click();
  await expect(toast(page, "Inbox marked as read.")).toBeVisible();
  expect(updates(server).flat().toSorted()).toEqual(unread.toSorted());
  await expect(header(page).getByRole("status")).toHaveText("Inbox marked as read.");
  // Nothing unread is left, so the name stands plain once the tree has it.
  await expect(header(page).getByRole("heading", { level: 1 })).toHaveText("Inbox");
  await expect(header(page).getByRole("button", { name: "Inbox" })).toHaveCount(0);
});

test("a job runs in rounds: the header shows its count with Cancel, which stops it after its round", async ({
  page,
}) => {
  const { server } = await writable(page, INBOX_PATH, ROUND_SIZE);
  server.holdSets = true;
  await header(page).getByRole("button", { name: "Inbox" }).click();
  await page.getByRole("menuitem", { name: "Mark all as read" }).click();
  await expect(header(page)).toContainText("Marking Inbox as read…");
  await expect.poll(() => server.heldSets.length).toBe(1);
  server.heldSets.shift()?.();
  await expect(header(page)).toContainText(`10 of ${String(INBOX_UNREAD)}`);
  await expect(header(page).getByRole("heading")).toHaveCount(0);
  await expect.poll(() => server.heldSets.length).toBe(1);
  await settled(page);
  for (const theme of THEMES) {
    await page.emulateMedia({ colorScheme: theme });
    await expect.soft(header(page)).toHaveScreenshot(`progress-${theme}-desktop.png`);
  }
  expect(await axeOn(page).include("main header").analyze()).toMatchObject({ violations: [] });
  await header(page).getByRole("button", { name: "Cancel" }).click();
  releaseSets(server);
  await expect(toast(page, `Stopped after 20 of ${String(INBOX_UNREAD)}.`)).toBeVisible();
  await expect(header(page).getByRole("heading", { level: 1 })).toBeVisible();
  expect(updates(server).map((ids) => ids.length)).toEqual([ROUND_SIZE, ROUND_SIZE]);
});

test("Empty Trash asks the dialog with Cancel first; the action destroys what Trash holds", async ({
  page,
}) => {
  const { server } = await writable(page, TRASH_PATH);
  await menuHolds(page, "Trash", ["Empty Trash"]);
  await page.getByRole("menuitem", { name: "Empty Trash" }).click();
  const dialog = page.getByRole("dialog", { name: "Empty Trash?" });
  await expect(dialog).toContainText(
    `${String(TRASH_TOTAL)} messages will be deleted forever. This can’t be undone.`,
  );
  await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();
  expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
  await settled(page);
  for (const theme of THEMES) {
    await page.emulateMedia({ colorScheme: theme });
    await expect.soft(dialog).toHaveScreenshot(`empty-dialog-${theme}-desktop.png`);
  }
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(header(page).getByRole("button", { name: "Trash" })).toBeFocused();
  expect(destroyed(server)).toEqual([]);
  await page.keyboard.press("Enter");
  await page.getByRole("menuitem", { name: "Empty Trash" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Empty Trash" }).click();
  await expect(toast(page, "Trash emptied.")).toBeVisible();
  await expect(page.getByText("Trash is empty.")).toBeVisible();
  expect(destroyed(server)).toHaveLength(TRASH_TOTAL);
});

test("a tree item opens the same menu by a right click and by the context-menu key", async ({
  page,
}) => {
  await writable(page, JUNK_PATH);
  const junk = page.getByRole("treeitem", { name: "Junk, 1 unread" });
  await junk.click({ button: "right" });
  const menu = page.getByRole("menu");
  await expect(menu.getByRole("menuitem")).toHaveText(["Mark all as read", "Empty Junk"]);
  await expect(menu.getByRole("separator")).toHaveCount(1);
  await settled(page);
  await expect.soft(page).toHaveScreenshot("tree-menu-light-desktop.png");
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
  await expect(junk).toBeFocused();
  await page.keyboard.press("ContextMenu");
  await expect(page.getByRole("menu")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(junk).toBeFocused();
  await page.getByRole("treeitem", { name: "Sent" }).click({ button: "right" });
  await expect(page.getByRole("menu")).toHaveCount(0);
});

test("offline a job runs nothing and the header says so until the connection is back", async ({
  page,
}) => {
  const { server } = await writable(page);
  await page.context().setOffline(true);
  await header(page).getByRole("button", { name: "Inbox" }).click();
  await page.getByRole("menuitem", { name: "Mark all as read" }).click();
  const sentence = "Offline: this will work again when the connection is back.";
  await expect(header(page)).toContainText(sentence);
  await expect(header(page).getByRole("status")).toHaveText(sentence);
  expect(server.sets).toEqual([]);
  await page.context().setOffline(false);
  await expect(header(page).getByRole("heading", { level: 1 })).toHaveText("Inbox");
});

test("a round the server takes nothing of ends the job with Try again, which runs it once more", async ({
  page,
}) => {
  const { server } = await writable(page);
  for (const id of unreadIds(server)) {
    server.refused.set(id, "forbidden");
  }
  await header(page).getByRole("button", { name: "Inbox" }).click();
  await page.getByRole("menuitem", { name: "Mark all as read" }).click();
  const failed = toast(page, "Couldn’t mark Inbox as read. Nothing was marked.");
  await expect(failed).toBeVisible();
  server.refused.clear();
  await failed.getByRole("button", { name: "Try again" }).click();
  await expect(toast(page, "Inbox marked as read.")).toBeVisible();
});

test("the menus, the dialog and the progress read right to left in the pseudo-locale", async ({
  page,
}) => {
  await page.setViewportSize(VIEWPORTS[1]);
  const { server } = await writable(page, INBOX_PATH, ROUND_SIZE);
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "en-XA");
  });
  await page.goto(INBOX_PATH);
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
  const menu = page.getByRole("menu");
  await header(page).getByRole("button", { name: "Inbox" }).click();
  await expect(menu.getByRole("menuitem")).toHaveCount(1);
  await settled(page);
  await expect.soft(page).toHaveScreenshot("folder-menu-en-XA-light-desktop.png");
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
  await page.getByRole("treeitem", { name: /^Junk/ }).focus();
  await page.keyboard.press("ContextMenu");
  await expect(menu.getByRole("menuitem")).toHaveCount(2);
  await settled(page);
  await expect.soft(page).toHaveScreenshot("tree-menu-en-XA-light-desktop.png");
  await menu.getByRole("menuitem", { name: /Émptý Júñk/ }).click();
  const dialog = page.getByRole("dialog", { name: /Émptý Júñk/ });
  await expect(dialog.getByRole("button", { name: /Çáñçél/ })).toBeFocused();
  await settled(page);
  await expect.soft(dialog).toHaveScreenshot("empty-dialog-en-XA-light-desktop.png");
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  server.holdSets = true;
  await header(page).getByRole("button", { name: "Inbox" }).click();
  await menu.getByRole("menuitem", { name: /Márk áll ás réád/ }).click();
  await expect.poll(() => server.heldSets.length).toBe(1);
  server.heldSets.shift()?.();
  await expect(header(page)).toContainText("10 óf");
  await expect.poll(() => server.heldSets.length).toBe(1);
  await settled(page);
  await expect.soft(header(page)).toHaveScreenshot("progress-en-XA-light-desktop.png");
  const cancel = header(page).getByRole("button", { name: /Çáñçél/ });
  await cancel.click();
  releaseSets(server);
  await expect(header(page).getByRole("heading", { level: 1 })).toBeVisible();
});

test.describe("on a phone", () => {
  test.use({ viewport: VIEWPORTS[0], hasTouch: true, isMobile: true });

  test("the name's menu holds Select first and the dialog fills the screen's width", async ({
    page,
  }) => {
    await writable(page, TRASH_PATH);
    await header(page).getByRole("button", { name: "Trash" }).click();
    await expect(page.getByRole("menuitem")).toHaveText(["Select", "Empty Trash"]);
    await settled(page);
    await expect.soft(page).toHaveScreenshot("folder-menu-light-phone.png");
    await page.getByRole("menuitem", { name: "Empty Trash" }).click();
    const dialog = page.getByRole("dialog", { name: "Empty Trash?" });
    await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();
    await settled(page);
    await expect.soft(page).toHaveScreenshot("empty-dialog-light-phone.png");
  });

  test("a long press on a tree item in the sidebar's sheet opens its menu, whose item runs the job", async ({
    page,
  }) => {
    await writable(page);
    await header(page).getByRole("button", { name: "Mailboxes and accounts" }).click();
    const junk = page.getByRole("treeitem", { name: "Junk, 1 unread" });
    // The sheet slides in; a trial click waits until the item stands still.
    await junk.click({ trial: true });
    const box = await junk.boundingBox();
    const at = { clientX: (box?.x ?? 0) + PRESS_INSET_PX, clientY: (box?.y ?? 0) + PRESS_INSET_PX };
    const touch = { pointerType: "touch", isPrimary: true, ...at };
    await junk.dispatchEvent("pointerdown", touch);
    await page.waitForTimeout(LONG_PRESS_HOLD_MS);
    await junk.dispatchEvent("pointerup", touch);
    await expect(page.getByRole("menu").getByRole("menuitem")).toHaveText([
      "Mark all as read",
      "Empty Junk",
    ]);
    await page.getByRole("menuitem", { name: "Mark all as read" }).click();
    await expect(toast(page, "Junk marked as read.")).toBeVisible();
    await expect(page).toHaveURL(INBOX_PATH);
  });
});
