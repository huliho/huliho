// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import type { MailServer } from "./mail-answers";
import { corpusFor } from "./mail-corpus";
import type { MailboxBody } from "./mail-mocks";
import { THEMES, VIEWPORTS, axeOn } from "./sweep";
import {
  INBOX_PATH,
  READ_INBOX,
  WRITABLE_READ_INBOX,
  grid,
  heightOf,
  openInbox,
  pane,
  rowAt,
  settled,
  subjectOf,
  threadIdOf,
  threadOf,
  updates,
} from "./thread-pane";

// The inbox as a shared mailbox a reader may neither mark nor move in.
function readOnly(row: MailboxBody): MailboxBody {
  return row.role === "inbox"
    ? { ...row, myRights: { ...row.myRights, maySetSeen: false, maySetKeywords: false } }
    : row;
}

const SHARED_MAILBOXES = READ_INBOX.map(readOnly);
const TRASH_ID = "mb-trash";
const TRASH_PATH = `/mail/acc-1/${TRASH_ID}`;
// The patch log waits this long for the changes behind the first.
const FLUSH_MS = 300;
// The toast's window, past which a held destroy goes out.
const UNDO_MS = 5_000;
const PHONE = { width: 390, height: 844 };
const TABLET = { width: 834, height: 1112 };
// The row's height at the compact and the touch density, so a shot
// named for a density shows that one.
const COMPACT_ROW_PX = 40;
const TOUCH_ROW_PX = 56;
// A list this wide shows the strip with its key caps.
const WIDE_LIST_PX = 520;
// A press this far down a compact row is inside a button's box and past
// the first line's text.
const LOW_PRESS_PX = 22;

// The destroy list of each Email/set that carried one.
function destroys(server: MailServer): unknown[] {
  return server.sets.flatMap((args) => (Array.isArray(args["destroy"]) ? [args["destroy"]] : []));
}

// Whether the address names the inbox's thread at `index`.
function atThread(index: number): (url: URL) => boolean {
  return (url) => url.pathname === `${INBOX_PATH}/${threadIdOf(index)}`;
}

function toast(page: Page, text: string) {
  return page.getByRole("dialog").filter({ hasText: text });
}

// A toolbar button of the open thread, by its name.
function tool(page: Page, name: string) {
  return pane(page).getByRole("button", { name, exact: true });
}

// The hint that stands open: its label with the key cap after it.
function hint(page: Page) {
  return page.locator("[data-side][data-open]").last();
}

// The second line of a row, where a press opens the thread whatever the
// first line holds under the pointer.
async function openRow(page: Page, index: number): Promise<void> {
  await rowAt(page, index).getByText(subjectOf(index), { exact: true }).click();
}

async function inbox(page: Page, preferences: Record<string, string> = {}) {
  return openInbox(page, "right", INBOX_PATH, { mailboxes: WRITABLE_READ_INBOX, preferences });
}

test("e archives the cursor's whole thread: the row leaves, the toast and the status region say so, z puts it back", async ({
  page,
}) => {
  const { server } = await inbox(page);
  const members = threadOf(1).map((email) => email.id);
  await rowAt(page, 1).focus();
  await page.keyboard.press("e");
  await expect(toast(page, "Archived.")).toBeVisible();
  // The toast and the list's status region.
  await expect(page.getByText("Archived.", { exact: true })).toHaveCount(2);
  await expect(rowAt(page, 1)).toContainText(subjectOf(2));
  await expect(rowAt(page, 1)).toBeFocused();
  await expect.poll(() => updates(server)).toEqual([members]);
  expect(server.corpus.emails.get(members[0] ?? "")?.mailboxId).toBe("mb-archive");
  await page.keyboard.press("z");
  await expect(rowAt(page, 1)).toContainText(subjectOf(1));
  await expect.poll(() => updates(server).length).toBe(2);
  expect(server.corpus.emails.get(members[0] ?? "")?.mailboxId).toBe("mb-inbox");
});

test("an action on the open thread opens the next one with the focus on its title", async ({
  page,
}) => {
  await inbox(page);
  await rowAt(page, 1).focus();
  await page.keyboard.press("Enter");
  await expect(pane(page).getByRole("heading", { level: 2, name: subjectOf(1) })).toBeFocused();
  await page.keyboard.press("e");
  await expect(page).toHaveURL(atThread(2));
  await expect(pane(page).getByRole("heading", { level: 2, name: subjectOf(2) })).toBeFocused();
  await page.goBack();
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-inbox$/);
});

test("an action from the row of the open thread opens the next one and leaves the focus on that row", async ({
  page,
}) => {
  await inbox(page);
  await rowAt(page, 1).focus();
  await page.keyboard.press("Enter");
  await expect(pane(page).getByRole("heading", { level: 2, name: subjectOf(1) })).toBeFocused();
  await rowAt(page, 1).focus();
  await page.keyboard.press("e");
  await expect(page).toHaveURL(atThread(2));
  await expect(rowAt(page, 1)).toContainText(subjectOf(2));
  await expect(rowAt(page, 1)).toBeFocused();
  const next = pane(page).getByRole("heading", { level: 2, name: subjectOf(2) });
  await expect(next).toBeVisible();
  await expect(next).not.toBeFocused();
});

test("under go back to the list the thread closes and the focus lands on the next row", async ({
  page,
}) => {
  await inbox(page, { afterAction: "list" });
  await rowAt(page, 1).focus();
  await page.keyboard.press("Enter");
  await page.keyboard.press("#");
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-inbox$/);
  await expect(pane(page)).toContainText("Select a conversation.");
  await expect(rowAt(page, 1)).toBeFocused();
  await expect(rowAt(page, 1)).toContainText(subjectOf(2));
  await expect(toast(page, "Deleted.")).toBeVisible();
});

test.describe("on a touchscreen", () => {
  test.use({ hasTouch: true, viewport: PHONE });

  test("on a phone the screen moves on to the next thread", async ({ page }) => {
    await inbox(page);
    await rowAt(page, 1).click();
    await expect(page).toHaveURL(atThread(1));
    await page.keyboard.press("e");
    await expect(page).toHaveURL(atThread(2));
    await expect(page.getByRole("heading", { level: 1, name: subjectOf(2) })).toBeFocused();
  });

  test("a row carries no hover actions and the screen's toolbar holds the actions as icons after the way back", async ({
    page,
  }) => {
    await inbox(page);
    await rowAt(page, 2).hover();
    await expect(rowAt(page, 2).getByRole("button").first()).toBeHidden();
    await rowAt(page, 1).click();
    const bar = page.getByRole("region", { name: "Conversation" });
    const archive = bar.getByRole("button", { name: "Archive", exact: true });
    await expect(archive).toBeVisible();
    // An icon alone, its word read out and never drawn.
    const box = await archive.boundingBox();
    expect(box?.width).toBe(box?.height);
    await settled(page);
    for (const theme of THEMES) {
      await page.emulateMedia({ colorScheme: theme });
      await expect.soft(page).toHaveScreenshot(`actions-screen-${theme}-phone.png`);
    }
  });
});

test("delete forever in Trash waits for its toast: z keeps the message, the window's end destroys it", async ({
  page,
}) => {
  const { server } = await openInbox(page, "right", TRASH_PATH, { mailboxes: WRITABLE_READ_INBOX });
  const first = await rowAt(page, 1).textContent();
  await rowAt(page, 1).focus();
  await page.keyboard.press("#");
  await expect(toast(page, "Deleted forever.")).toBeVisible();
  await page.keyboard.press("z");
  await expect(rowAt(page, 1)).toHaveText(first ?? "");
  await page.waitForTimeout(FLUSH_MS * 2);
  expect(destroys(server)).toEqual([]);
  await page.keyboard.press("#");
  await expect(toast(page, "Deleted forever.")).toBeVisible();
  await expect.poll(() => destroys(server).length, { timeout: UNDO_MS * 2 }).toBe(1);
});

test("a refused archive puts the thread back and says the mail is safe", async ({ page }) => {
  const mail = await inbox(page);
  for (const email of threadOf(1)) {
    mail.server.refused.set(email.id, "forbidden");
  }
  await rowAt(page, 1).focus();
  await page.keyboard.press("e");
  await expect(toast(page, "Couldn’t archive. Your mail is safe.")).toBeVisible();
  await expect(toast(page, "Archived.")).toHaveCount(0);
  await expect(rowAt(page, 1)).toContainText(subjectOf(1));
  expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
});

test("the shortcut overlay lists the actions the inbox offers, and the flips change in place", async ({
  page,
}) => {
  const { server } = await inbox(page);
  await rowAt(page, 1).focus();
  await page.keyboard.press("s");
  await page.keyboard.press("u");
  await expect.poll(() => server.sets.length).toBeGreaterThan(0);
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await page.keyboard.press("?");
  const overlay = page.getByRole("dialog", { name: "Keyboard" });
  // The row starred and marked unread, so the flips offer their other side.
  for (const label of ["Archive", "Delete", "Mark as spam", "Mark as read", "Remove star"]) {
    await expect(overlay).toContainText(label);
  }
});

test("a mailbox that gives up nothing offers no action key", async ({ page }) => {
  const { server } = await openInbox(page);
  await rowAt(page, 1).focus();
  await page.keyboard.press("e");
  await page.keyboard.press("#");
  await page.waitForTimeout(FLUSH_MS * 2);
  expect(server.sets).toEqual([]);
  await expect(rowAt(page, 1)).toContainText(subjectOf(1));
});

// The row under the pointer draws no strip and keeps its time in sight.
async function keepsTime(page: Page, index: number): Promise<void> {
  const row = rowAt(page, index);
  const drawn = await row.innerText();
  await row.hover();
  await expect(row.getByRole("button")).toHaveCount(0);
  await expect(row).toHaveText(drawn, { useInnerText: true });
}

// The thread of Trash's first row, which the inbox's list never holds.
function trashThreadId(): string {
  const corpus = corpusFor(READ_INBOX);
  const exemplar = corpus.lists.get(TRASH_ID)?.exemplars[0] ?? "";
  return corpus.emails.get(exemplar)?.threadId ?? "";
}

test("a row of a mailbox that offers no action keeps its time under the pointer", async ({
  page,
}) => {
  await openInbox(page, "right", INBOX_PATH, { mailboxes: SHARED_MAILBOXES });
  await keepsTime(page, 3);
});

test("a row keeps its time under the pointer while the open thread has no message left in the mailbox shown", async ({
  page,
}) => {
  await openInbox(page, "right", `${INBOX_PATH}/${trashThreadId()}`);
  await expect(pane(page).getByRole("heading", { level: 2 })).toBeVisible();
  await keepsTime(page, 3);
});

test("a press in the toolbar takes the open thread, not the cursor's row, and the next one opens on its title", async ({
  page,
}) => {
  const { server } = await inbox(page);
  await openRow(page, 1);
  await expect(page).toHaveURL(atThread(1));
  await rowAt(page, 3).focus();
  await tool(page, "Archive").click();
  await expect(toast(page, "Archived.")).toBeVisible();
  await expect(page).toHaveURL(atThread(2));
  await expect(pane(page).getByRole("heading", { level: 2, name: subjectOf(2) })).toBeFocused();
  await expect.poll(() => updates(server)).toEqual([threadOf(1).map((email) => email.id)]);
});

test("a toolbar button names its label and its key by hover and by focus; Escape closes the hint before the thread", async ({
  page,
}) => {
  await inbox(page);
  await openRow(page, 1);
  await tool(page, "Mark as spam").hover();
  await expect(hint(page)).toHaveText("Mark as spam!");
  await page.mouse.move(0, 0);
  await expect(hint(page)).toBeHidden();
  await tool(page, "Archive").focus();
  await page.keyboard.press("Tab");
  await expect(tool(page, "Delete")).toBeFocused();
  await expect(hint(page)).toHaveText("Delete#");
  expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
  await page.keyboard.press("Escape");
  await expect(hint(page)).toBeHidden();
  await expect(page).toHaveURL(atThread(1));
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-inbox$/);
});

test("the toolbar draws its words per mailbox and leaves them to the tooltips where the pane is narrow", async ({
  page,
}) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await inbox(page);
  await openRow(page, 1);
  // The words drawn beside the icon make a labeled button wider than tall.
  const labeled = await tool(page, "Archive").boundingBox();
  expect(labeled?.width ?? 0).toBeGreaterThan(labeled?.height ?? 0);
  await settled(page);
  for (const theme of THEMES) {
    await page.emulateMedia({ colorScheme: theme });
    await expect
      .soft(pane(page).locator("div").first())
      .toHaveScreenshot(`toolbar-inbox-${theme}-desktop.png`);
  }
  await page.emulateMedia({ colorScheme: "light" });
  await page.goto(TRASH_PATH);
  await rowAt(page, 1).focus();
  await page.keyboard.press("Enter");
  await expect(tool(page, "Delete forever")).toBeVisible();
  await expect(tool(page, "Archive")).toHaveCount(0);
  await expect
    .soft(pane(page).locator("div").first())
    .toHaveScreenshot("toolbar-trash-light-desktop.png");
  await page.setViewportSize(TABLET);
  await tool(page, "Delete forever").hover();
  await expect(hint(page)).toHaveText("Delete forever#");
  const icon = await tool(page, "Delete forever").boundingBox();
  expect(icon?.width).toBe(icon?.height);
  await expect.soft(page).toHaveScreenshot("toolbar-light-tablet.png");
});

test("the row under the pointer shows its actions as icons in the time's place; a press on one acts on that row alone, one at the row's middle opens it", async ({
  page,
}) => {
  const { server } = await inbox(page);
  await rowAt(page, 1).focus();
  const third = rowAt(page, 3);
  await third.hover();
  const archive = third.getByRole("button", { name: "Archive", exact: true });
  await expect(archive.locator("kbd")).toBeHidden();
  await expect(third.getByRole("button")).toHaveCount(4);
  await settled(page);
  for (const theme of THEMES) {
    await page.emulateMedia({ colorScheme: theme });
    await expect.soft(grid(page)).toHaveScreenshot(`row-hover-${theme}-desktop.png`);
  }
  expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
  await archive.click();
  await expect(toast(page, "Archived.")).toBeVisible();
  await expect(page).toHaveURL(INBOX_PATH);
  await expect(rowAt(page, 1)).toBeFocused();
  await expect(rowAt(page, 3)).toContainText(subjectOf(4));
  await expect.poll(() => updates(server)).toEqual([threadOf(3).map((email) => email.id)]);
  // The middle of the row's first line, where the strip would stand if
  // it crossed the row's middle.
  const box = await third.boundingBox();
  await third.click({ position: { x: (box?.width ?? 0) / 2, y: (box?.height ?? 0) / 4 } });
  await expect(page).toHaveURL(atThread(4));
});

test("a list wide enough for the caps draws the keys beside the icons on the hovered row", async ({
  page,
}) => {
  await page.addInitScript((width: number) => {
    window.localStorage.setItem("huliho-list-width", String(width));
  }, WIDE_LIST_PX);
  await inbox(page);
  await rowAt(page, 1).focus();
  const third = rowAt(page, 3);
  await third.hover();
  const archive = third.getByRole("button", { name: "Archive", exact: true });
  await expect(archive.locator("kbd")).toBeVisible();
  await expect(archive).toHaveText("e");
  await settled(page);
  await expect.soft(grid(page)).toHaveScreenshot("row-hover-wide-light-desktop.png");
});

// A row by its index where the grid's name wears the pseudo-locale's accents.
function rowIn(page: Page, index: number) {
  return page.getByRole("grid").locator(`[aria-rowindex="${String(index)}"]`);
}

test("the labeled toolbar and the hovered row read right to left in the pseudo-locale", async ({
  page,
}) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await inbox(page);
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "en-XA");
  });
  await page.goto(INBOX_PATH);
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
  await rowIn(page, 1).focus();
  await rowIn(page, 3).hover();
  await expect(rowIn(page, 3).getByRole("button")).toHaveCount(4);
  await settled(page);
  await expect.soft(page.getByRole("grid")).toHaveScreenshot("row-hover-en-XA-light-desktop.png");
  await rowIn(page, 1).getByText(subjectOf(1), { exact: true }).click();
  const pseudo = page.getByRole("complementary");
  await expect(pseudo.getByRole("heading", { level: 2 })).toHaveText(subjectOf(1));
  const archive = await pseudo.getByRole("button", { name: /Árçhívé/ }).boundingBox();
  expect(archive?.width ?? 0).toBeGreaterThan(archive?.height ?? 0);
  await settled(page);
  await expect
    .soft(pseudo.locator("div").first())
    .toHaveScreenshot("toolbar-en-XA-light-desktop.png");
});

test("the hovered row keeps its strip at the compact and the touch density", async ({ page }) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await inbox(page, { density: "compact" });
  await expect.poll(() => heightOf(rowAt(page, 1))).toBe(COMPACT_ROW_PX);
  await rowAt(page, 1).focus();
  await rowAt(page, 3).hover();
  await expect(rowAt(page, 3).getByRole("button")).toHaveCount(4);
  await settled(page);
  await expect.soft(grid(page)).toHaveScreenshot("row-hover-compact-light-desktop.png");
  // A device with a touchscreen and a mouse gets the touch tokens and
  // keeps its hover, so the strip shows in the taller row. The attribute
  // sets those tokens on the document.
  await page.evaluate(() => {
    document.documentElement.dataset["density"] = "touch";
  });
  await expect.poll(() => heightOf(rowAt(page, 1))).toBe(TOUCH_ROW_PX);
  await rowAt(page, 3).hover();
  await expect(rowAt(page, 3).getByRole("button")).toHaveCount(4);
  await settled(page);
  await expect.soft(grid(page)).toHaveScreenshot("row-hover-touch-light-desktop.png");
});

test("at compact a press low in a hovered button's box takes its action", async ({ page }) => {
  await page.setViewportSize(VIEWPORTS[1]);
  const { server } = await inbox(page, { density: "compact" });
  await expect.poll(() => heightOf(rowAt(page, 1))).toBe(COMPACT_ROW_PX);
  const third = rowAt(page, 3);
  await third.hover();
  const remove = third.getByRole("button", { name: "Delete", exact: true });
  const hit = await remove.boundingBox();
  const box = await third.boundingBox();
  await third.click({
    position: { x: (hit?.x ?? 0) - (box?.x ?? 0) + (hit?.width ?? 0) / 2, y: LOW_PRESS_PX },
  });
  await expect(toast(page, "Deleted.")).toBeVisible();
  await expect.poll(() => updates(server)).toEqual([threadOf(3).map((email) => email.id)]);
});
