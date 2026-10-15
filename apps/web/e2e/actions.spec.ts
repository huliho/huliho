// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import type { MailServer } from "./mail-answers";
import { WRITABLE_MAILBOXES } from "./mail-mocks";
import type { MailboxBody } from "./mail-mocks";
import { axeOn } from "./sweep";
import { INBOX_PATH, openInbox, pane, rowAt, subjectOf, threadIdOf, threadOf } from "./thread-pane";

function allRead(row: MailboxBody): MailboxBody {
  return row.role === "inbox" ? { ...row, unreadEmails: 0 } : row;
}

// Every message read, as the thread pane's inbox has it, on a server
// that moves messages.
const MAILBOXES = WRITABLE_MAILBOXES.map(allRead);
const TRASH_PATH = "/mail/acc-1/mb-trash";
// The patch log waits this long for the changes behind the first.
const FLUSH_MS = 300;
// The toast's window, past which a held destroy goes out.
const UNDO_MS = 5_000;
const PHONE = { width: 390, height: 844 };

// Every email id each Email/set named in its update, and each destroy.
function updates(server: MailServer): string[][] {
  return server.sets.map(({ update }) =>
    typeof update === "object" && update !== null ? Object.keys(update) : [],
  );
}

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

async function inbox(page: Page, preferences: Record<string, string> = {}) {
  return openInbox(page, "right", INBOX_PATH, { mailboxes: MAILBOXES, preferences });
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

test("on a phone the screen moves on to the next thread", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await inbox(page);
  await rowAt(page, 1).click();
  await expect(page).toHaveURL(atThread(1));
  await page.keyboard.press("e");
  await expect(page).toHaveURL(atThread(2));
  await expect(page.getByRole("heading", { level: 1, name: subjectOf(2) })).toBeFocused();
});

test("delete forever in Trash waits for its toast: z keeps the message, the window's end destroys it", async ({
  page,
}) => {
  const { server } = await openInbox(page, "right", TRASH_PATH, { mailboxes: MAILBOXES });
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
