// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Locator, Page } from "@playwright/test";
import { expect } from "@playwright/test";

import { accountRow, mockAccounts } from "./account-mocks";
import { FIXED_NOW, corpusFor } from "./mail-corpus";
import type { CorpusEmail } from "./mail-corpus";
import { MAILBOXES, mockMail } from "./mail-mocks";
import type { MailboxBody } from "./mail-mocks";
import { mockPreferences } from "./preference-mocks";
import { mockSignedIn } from "./session-mocks";

const INBOX_ID = "mb-inbox";
export const INBOX_PATH = "/mail/acc-1/mb-inbox";
export const ROWS = [accountRow(FIXED_NOW)];
// The collapsed cards in sight beside the open one.
export const CARDS_IN_SIGHT = 3;

function withEveryMessageRead(row: MailboxBody): MailboxBody {
  return { ...row, unreadEmails: 0 };
}

// An inbox with every message read, so the first thread of four keeps
// a card behind the pane's button and stands among the rows in view.
export const READ_INBOX: MailboxBody[] = MAILBOXES.map((row) =>
  row.id === INBOX_ID ? withEveryMessageRead(row) : row,
);
const CORPUS = corpusFor(READ_INBOX);
const INBOX_LIST = CORPUS.lists.get(INBOX_ID) ?? { ids: [], exemplars: [] };

// The messages of the row's thread, oldest first.
export function threadOf(rowIndex: number): CorpusEmail[] {
  const id = INBOX_LIST.exemplars.at(rowIndex - 1) ?? "";
  const ids = CORPUS.threads.get(CORPUS.emails.get(id)?.threadId ?? "") ?? [];
  return ids.flatMap((memberId) => CORPUS.emails.get(memberId) ?? []);
}

export function threadSizeOf(rowIndex: number): number {
  return threadOf(rowIndex).length;
}

// The first row whose thread passes the test.
function rowOf(fits: (thread: CorpusEmail[]) => boolean): number {
  const at = INBOX_LIST.exemplars.findIndex((_, index) => fits(threadOf(index + 1)));
  if (at < 0) {
    throw new Error("the corpus has no such thread");
  }
  return at + 1;
}

// A thread with a message behind the pane's button; a thread of one.
export const LONG_ROW = rowOf((thread) => thread.length > CARDS_IN_SIGHT);
export const SINGLE_ROW = rowOf((thread) => thread.length === 1);

function exemplarOf(rowIndex: number): CorpusEmail | undefined {
  const id = INBOX_LIST.exemplars.at(rowIndex - 1);
  return id === undefined ? undefined : CORPUS.emails.get(id);
}

export function subjectOf(rowIndex: number): string {
  return exemplarOf(rowIndex)?.subject ?? "";
}

export function threadIdOf(rowIndex: number): string {
  return exemplarOf(rowIndex)?.threadId ?? "";
}

export type Position = "right" | "bottom" | "off";

// The signed-in tab at its first address: the inbox, or a thread's own.
export async function openInbox(
  page: Page,
  readingPane: Position = "right",
  path: string = INBOX_PATH,
): Promise<void> {
  await mockSignedIn(page);
  await mockMail(page, READ_INBOX);
  await mockAccounts(page, ROWS);
  await mockPreferences(page, { readingPane });
  await page.clock.setFixedTime(FIXED_NOW);
  await page.goto(path);
  await expect(rowAt(page, 1)).toBeVisible();
}

export function grid(page: Page): Locator {
  return page.getByRole("grid", { name: "Conversations" });
}

export function rowAt(page: Page, index: number): Locator {
  return grid(page).locator(`[aria-rowindex="${String(index)}"]`);
}

export function pane(page: Page): Locator {
  return page.getByRole("complementary", { name: "Conversation" });
}

export function screenOf(page: Page): Locator {
  return page.getByRole("region", { name: "Conversation" });
}

export async function heightOf(locator: Locator): Promise<number> {
  const box = await locator.boundingBox();
  return box?.height ?? 0;
}

// The direction the element lays its text out in, as the browser resolved it.
export async function directionOf(locator: Locator): Promise<string> {
  return locator.evaluate((element) => getComputedStyle(element).direction);
}

// The fonts in and every animation over, so a screenshot is still.
export async function settled(page: Page): Promise<void> {
  await page.evaluate(async () => {
    await document.fonts.ready;
    await Promise.all(document.getAnimations().map((animation) => animation.finished));
  });
}
