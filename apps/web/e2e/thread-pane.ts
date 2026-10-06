// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Locator, Page } from "@playwright/test";
import { expect } from "@playwright/test";

import { accountRow, mockAccounts } from "./account-mocks";
import { FIXED_NOW, corpusFor } from "./mail-corpus";
import type { CorpusEmail } from "./mail-corpus";
import { MAILBOXES, mockMail } from "./mail-mocks";
import type { MailOptions, MailboxBody, MockedMail } from "./mail-mocks";
import { mockPreferences } from "./preference-mocks";
import { mockSignedIn } from "./session-mocks";
import { framesLoaded } from "./sweep";

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

// The first row whose newest message the sender wrote.
export function rowFrom(sender: string): number {
  return rowOf((thread) => thread.at(-1)?.from.email === sender);
}

// The first row that is one message alone, from the sender.
export function singleRowFrom(sender: string): number {
  return rowOf((thread) => thread.length === 1 && thread[0]?.from.email === sender);
}

// The newest message of the row's thread, which its open card shows.
export function newestOf(rowIndex: number): CorpusEmail | undefined {
  return threadOf(rowIndex).at(-1);
}

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

// The address of the row's thread.
export function pathOf(rowIndex: number): string {
  return `${INBOX_PATH}/${threadIdOf(rowIndex)}`;
}

export type Position = "right" | "bottom" | "off";

export interface InboxExtras {
  // The mailboxes behind the tab; every message read unless given.
  mailboxes?: MailboxBody[];
  mail?: MailOptions;
  // Leaves the page its own clock. The fixed one installs a script in
  // every document, which a message's sandboxed frame refuses out loud.
  ownClock?: boolean;
}

// The signed-in tab at its first address: the inbox, or a thread's own.
export async function openInbox(
  page: Page,
  readingPane: Position = "right",
  path: string = INBOX_PATH,
  extras: InboxExtras = {},
): Promise<MockedMail> {
  await mockSignedIn(page);
  const mail = await mockMail(page, extras.mailboxes ?? READ_INBOX, extras.mail ?? {});
  await mockAccounts(page, ROWS);
  await mockPreferences(page, { readingPane });
  if (extras.ownClock !== true) {
    await page.clock.setFixedTime(FIXED_NOW);
  }
  await page.goto(path);
  await expect(rowAt(page, 1)).toBeVisible();
  return mail;
}

// The cards of the open thread, oldest first; the newest is the last.
export function cards(page: Page): Locator {
  return page.getByRole("listitem");
}

// The frame a card renders its message in.
export function frameIn(card: Locator): Locator {
  return card.locator("iframe");
}

// Every open card of the thread in view shows its message: the newest
// card stands open, no still lines stand in a card and every frame has
// loaded. Called once the thread's own title shows.
export async function bodiesLanded(page: Page): Promise<void> {
  const fold = cards(page).last().getByRole("button").first();
  await expect(fold).toHaveAttribute("aria-expanded", "true");
  await expect(cards(page).getByRole("status", { name: "Loading…" })).toHaveCount(0);
  await framesLoaded(page);
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
