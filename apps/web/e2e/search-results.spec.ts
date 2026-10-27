// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Locator, Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { accountRow, mockAccounts } from "./account-mocks";
import { FIXED_NOW, corpusFor, seedThread } from "./mail-corpus";
import type { Corpus } from "./mail-corpus";
import { WRITABLE_MAILBOXES, mockMail } from "./mail-mocks";
import { mockSignedIn } from "./session-mocks";
import { axeOn } from "./sweep";
import { settled } from "./thread-pane";

const WORD = "zonnepanelen";
const PHONE = { width: 390, height: 844 };
// The search over every mailbox but Junk and Trash, opened from the inbox.
const SPREAD = `/mail/acc-1/search?q=${WORD}&box=mb-inbox`;
// The same search kept to the inbox.
const INBOX_ONLY = `/mail/acc-1/search?q=${WORD}&in=mb-inbox&box=mb-inbox`;
// The two inbox threads, newest first.
const NEWEST = "mb-inbox-tx1";
const NEXT = "mb-inbox-tx0";

// Two inbox threads and one in the archive name the word, so does one
// in Junk and one in Trash, each a day apart, on a server that moves mail.
function corpus(): Corpus {
  const empty = WRITABLE_MAILBOXES.map((row) => ({ ...row, totalEmails: 0, unreadEmails: 0 }));
  const built = corpusFor(empty);
  seedThread(built, "mb-trash", [{ subject: "Oude zonnepanelen" }], new Date("2026-05-09"));
  seedThread(built, "mb-junk", [{ subject: "Goedkope zonnepanelen" }], new Date("2026-05-10"));
  seedThread(built, "mb-archive", [{ subject: "Zonnepanelen offerte" }], new Date("2026-05-11"));
  seedThread(built, "mb-inbox", [{ subject: "Zonnepanelen planning" }], new Date("2026-05-12"));
  seedThread(built, "mb-inbox", [{ subject: "Zonnepanelen factuur" }], new Date("2026-05-13"));
  return built;
}

async function searched(page: Page, path: string): Promise<void> {
  await mockSignedIn(page);
  await mockMail(page, WRITABLE_MAILBOXES, { corpus: corpus() });
  await mockAccounts(page, [accountRow(FIXED_NOW)]);
  await page.clock.setFixedTime(FIXED_NOW);
  await page.goto(path);
}

function rows(page: Page): Locator {
  return page.getByRole("main").getByRole("grid", { name: "Conversations" }).getByRole("row");
}

function spoken(page: Page): Locator {
  return page.getByRole("search").getByRole("status");
}

test("results stand in the list with the mailbox each acts from; one opens at the search's address and closes back to the results", async ({
  page,
}) => {
  await searched(page, SPREAD);
  await expect(rows(page)).toHaveCount(3);
  await expect(rows(page).nth(0)).toHaveAccessibleName(/, in Inbox$/u);
  await expect(rows(page).nth(2)).toHaveAccessibleName(/, in Archive$/u);
  await expect(rows(page).nth(2).getByText("Archive", { exact: true })).toBeVisible();
  // The shortcuts stay away over the results, so a press on a row moves nothing.
  await expect(page.getByRole("combobox", { name: "Search mail" })).toBeFocused();
  await expect(page.getByRole("button", { name: "from: a sender" })).toHaveCount(0);
  expect((await axeOn(page).analyze()).violations).toEqual([]);
  await rows(page).nth(0).click();
  await expect(page).toHaveURL(
    new RegExp(`/mail/acc-1/search/${NEWEST}\\?q=${WORD}&box=mb-inbox&at=mb-inbox$`, "u"),
  );
  await expect(page.getByRole("heading", { level: 2, name: "Zonnepanelen factuur" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(new RegExp(`/mail/acc-1/search\\?q=${WORD}&box=mb-inbox$`, "u"));
  await expect(rows(page).nth(0)).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-inbox$/u);
});

test("an archive from every mailbox but Junk and Trash keeps the row with its mailbox renewed and opens the next result", async ({
  page,
}) => {
  await searched(page, SPREAD);
  await rows(page).nth(0).click();
  await expect(page.getByRole("heading", { level: 2, name: "Zonnepanelen factuur" })).toBeFocused();
  await page.keyboard.press("e");
  await expect(page).toHaveURL(new RegExp(`/search/${NEXT}\\?`, "u"));
  await expect(rows(page)).toHaveCount(3);
  await expect(rows(page).nth(0)).toHaveAccessibleName(/^.*Zonnepanelen factuur.*, in Archive$/u);
  await expect(
    page.getByRole("heading", { level: 2, name: "Zonnepanelen planning" }),
  ).toBeFocused();
});

test("under the inbox alone an archive takes the row out and the count drops; z puts it back", async ({
  page,
}) => {
  await searched(page, INBOX_ONLY);
  await expect(rows(page)).toHaveCount(2);
  await expect(rows(page).nth(0)).not.toHaveAccessibleName(/, in /u);
  await rows(page).nth(0).focus();
  await page.keyboard.press("e");
  await expect(rows(page)).toHaveCount(1);
  await expect(spoken(page)).toHaveText(`1 result for “${WORD}” in Inbox`);
  await page.keyboard.press("z");
  await expect(rows(page)).toHaveCount(2);
});

test("a delete takes the row out of every mailbox but Junk and Trash", async ({ page }) => {
  await searched(page, SPREAD);
  await rows(page).nth(1).focus();
  await page.keyboard.press("#");
  await expect(rows(page)).toHaveCount(2);
  await expect(spoken(page)).toHaveText(`2 results for “${WORD}” in all mailboxes`);
});

test("a bulk archive on results moves the picks and its undo puts them back", async ({ page }) => {
  await searched(page, SPREAD);
  await rows(page).nth(0).focus();
  await page.keyboard.press("x");
  await rows(page).nth(1).focus();
  await page.keyboard.press("x");
  await expect(page.getByRole("toolbar", { name: "2 selected" })).toBeVisible();
  await page.keyboard.press("e");
  const toast = page.getByRole("dialog").filter({ hasText: "2 conversations archived." });
  await expect(toast).toBeVisible();
  await expect(rows(page).nth(0)).toHaveAccessibleName(/, in Archive$/u);
  await expect(rows(page).nth(1)).toHaveAccessibleName(/, in Archive$/u);
  await toast.getByRole("button", { name: "Undo" }).click();
  await expect(rows(page).nth(0)).toHaveAccessibleName(/, in Inbox$/u);
  await expect(rows(page).nth(1)).toHaveAccessibleName(/, in Inbox$/u);
});

test("on a phone a result opens as its own screen whose way back reads Results", async ({
  page,
}) => {
  await page.setViewportSize(PHONE);
  await searched(page, SPREAD);
  await rows(page).nth(2).click();
  const back = page.getByRole("button", { name: "Back to results" });
  await expect(back).toHaveText("Results");
  await expect(page.getByRole("heading", { level: 1, name: "Zonnepanelen offerte" })).toBeVisible();
  await settled(page);
  expect((await axeOn(page).analyze()).violations).toEqual([]);
  await back.click();
  await expect(page).toHaveURL(new RegExp(`/mail/acc-1/search\\?q=${WORD}&box=mb-inbox$`, "u"));
  await expect(rows(page)).toHaveCount(3);
});
