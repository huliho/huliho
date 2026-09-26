// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { AxeBuilder } from "@axe-core/playwright";
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { STORYBOOK_URL } from "../playwright.config";
import { accountRow, mockAccounts } from "./account-mocks";
import { FIXED_NOW, corpusFor } from "./mail-corpus";
import { MAILBOXES, mockMail, refuseListWhile } from "./mail-mocks";
import type { MailOptions, MailboxBody, MockedMail } from "./mail-mocks";
import { mockPreferences } from "./preference-mocks";
import { mockSignedIn } from "./session-mocks";
import { THEMES, VIEWPORTS, WCAG_TAGS } from "./sweep";

const INBOX_ID = "mb-inbox";
const ROWS = [accountRow(FIXED_NOW)];
// The worker skips a focus poll this soon after the last one; the test
// waits past it before it asks for one.
const FOCUS_POLL_GAP_MS = 10_000;
const PAST_FOCUS_GAP_MS = FOCUS_POLL_GAP_MS + 1_000;
// A bridge inbox a little way into its first sync.
const SYNCED = 40;
const SYNCING: MailboxBody[] = MAILBOXES.map((row) =>
  row.id === INBOX_ID ? { ...row, syncedEmails: SYNCED } : row,
);
// The first message sits one step before the fixed now.
const FIRST_TIME = "9:37 AM";
const FIRST_TIME_NL = "09:37";
// A spot inside the reading pane at the desktop width, where nothing under the pointer lights up.
const RESTING_POINT = { x: 1024, y: 450 };
const TRANSPARENT = "rgba(0, 0, 0, 0)";
// The drawn row states, in the story's order.
const DEFAULT_ROW = 0;
const MULTI_SELECTED_ROW = 5;

async function openInbox(
  page: Page,
  mailboxes: MailboxBody[] = MAILBOXES,
  options: MailOptions = {},
): Promise<MockedMail> {
  await mockSignedIn(page);
  const mail = await mockMail(page, mailboxes, options);
  await mockAccounts(page, ROWS);
  await page.clock.setFixedTime(FIXED_NOW);
  await page.goto("/mail/acc-1/mb-inbox");
  return mail;
}

function grid(page: Page): Locator {
  return page.getByRole("grid", { name: "Conversations" });
}

function rowAt(page: Page, index: number): Locator {
  return grid(page).locator(`[aria-rowindex="${String(index)}"]`);
}

// The end edge of an element's box on the block axis, in whole pixels.
async function bottomOf(locator: Locator): Promise<number> {
  const box = await locator.boundingBox();
  if (box === null) {
    throw new Error("the element has no box");
  }
  return Math.round(box.y + box.height);
}

// The corpus the default mailboxes get, as the mock builds it.
const CORPUS = corpusFor(MAILBOXES);
const INBOX_LIST = CORPUS.lists.get(INBOX_ID) ?? { ids: [], exemplars: [] };

function exemplar(at: number) {
  const id = INBOX_LIST.exemplars.at(at);
  const message = id === undefined ? undefined : CORPUS.emails.get(id);
  if (message === undefined) {
    throw new Error(`the corpus has no exemplar ${String(at)}`);
  }
  return message;
}

test("the inbox lists its threads newest first, each row named for a screen reader", async ({
  page,
}) => {
  await openInbox(page);
  await expect(grid(page)).toHaveAttribute("aria-rowcount", String(INBOX_LIST.exemplars.length));
  const first = exemplar(0);
  await expect(rowAt(page, 1)).toHaveAccessibleName(
    `${first.from.name}, ${first.subject}, ${FIRST_TIME}, unread`,
  );
  await expect(rowAt(page, 1)).toHaveAttribute("tabindex", "0");
  await expect(rowAt(page, 2)).toHaveAttribute("tabindex", "-1");
  const threaded = INBOX_LIST.exemplars.findIndex(
    (id) => (CORPUS.threads.get(CORPUS.emails.get(id)?.threadId ?? "")?.length ?? 0) > 1,
  );
  const size = CORPUS.threads.get(exemplar(threaded).threadId)?.length ?? 0;
  expect(await rowAt(page, threaded + 1).getAttribute("aria-label")).toContain(
    `, ${String(size)} messages`,
  );
  await expect(rowAt(page, threaded + 1)).toContainText(String(size));
  expect(await grid(page).getByRole("row").count()).toBeLessThan(INBOX_LIST.exemplars.length);
});

test("the arrow keys, j and k walk the rows, Home and End jump and Tab leaves the grid", async ({
  page,
}) => {
  await openInbox(page);
  const last = INBOX_LIST.exemplars.length;
  await rowAt(page, 1).focus();
  await page.keyboard.press("ArrowDown");
  await expect(rowAt(page, 2)).toBeFocused();
  await page.keyboard.press("j");
  await expect(rowAt(page, 3)).toBeFocused();
  await page.keyboard.press("k");
  await expect(rowAt(page, 2)).toBeFocused();
  await page.keyboard.press("End");
  await expect(rowAt(page, last)).toBeFocused();
  await expect(rowAt(page, last)).not.toHaveAccessibleName("Loading…");
  await page.keyboard.press("ArrowUp");
  await expect(rowAt(page, last - 1)).toBeFocused();
  await page.keyboard.press("Home");
  await expect(rowAt(page, 1)).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(page.getByRole("separator", { name: "Resize the list" })).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(rowAt(page, 1)).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(page.getByRole("treeitem", { name: "Inbox, 23 unread" })).toBeFocused();
});

test("new mail waits above the list as a marker until the dot key brings it in", async ({
  page,
}) => {
  const mail = await openInbox(page);
  const before = await rowAt(page, 1).getAttribute("aria-label");
  const arrived = mail.arrive(INBOX_ID);
  await page.waitForTimeout(PAST_FOCUS_GAP_MS);
  await page.evaluate(() => {
    document.dispatchEvent(new Event("visibilitychange"));
  });
  const marker = page.getByRole("button", { name: "1 new message" });
  await expect(marker).toBeVisible();
  await expect(rowAt(page, 1)).toHaveAttribute("aria-label", before ?? "");
  await page.keyboard.press(".");
  await expect(marker).toBeHidden();
  await expect
    .poll(() => rowAt(page, 1).getAttribute("aria-label"))
    .toContain(`${arrived.from.name}, ${arrived.subject}, `);
});

test("offline shows the strip over the list and the rows stay", async ({ page, context }) => {
  await openInbox(page);
  await expect(rowAt(page, 1)).toBeVisible();
  await context.setOffline(true);
  const strip = page.getByRole("status").filter({ hasText: "Offline: showing cached mail." });
  await expect(strip).toBeVisible();
  await expect(rowAt(page, 1)).toBeVisible();
  await context.setOffline(false);
  await expect(strip).toBeHidden();
});

test("a first sync shows its progress at the foot with still rows after the last synced one", async ({
  page,
}) => {
  await openInbox(page, SYNCING, { vendor: true });
  const foot = page.getByRole("status").filter({ hasText: "Syncing this mailbox" });
  await expect(foot).toHaveText("Syncing this mailbox for the first time.");
  await expect(page.getByText("40 of 1,204")).toBeVisible();
  const synced = corpusFor(SYNCING).lists.get(INBOX_ID)?.exemplars.length ?? 0;
  await expect(grid(page)).toHaveAttribute("aria-rowcount", String(synced));
  await rowAt(page, 1).focus();
  await page.keyboard.press("End");
  await expect(rowAt(page, synced)).toBeFocused();
  await expect(grid(page).locator('[aria-hidden="true"]').first()).toBeVisible();
});

test("a list that fails to load says so while the tree stands, and Try again brings it", async ({
  page,
}) => {
  await page.setViewportSize(VIEWPORTS[1]);
  let failing = true;
  await mockSignedIn(page);
  await mockMail(page);
  await mockAccounts(page, ROWS);
  await refuseListWhile(page, () => failing);
  await page.goto("/mail/acc-1/mb-inbox");
  await expect(page.getByRole("alert")).toContainText("Couldn’t load your mail.");
  await expect(page.getByRole("treeitem")).toHaveCount(MAILBOXES.length);
  // The key hints keep the foot of the pane, under the error box.
  const main = page.getByRole("main");
  const strip = main.locator("p", { hasText: "shortcuts" });
  await expect(strip).toBeVisible();
  expect(await bottomOf(strip)).toBe(await bottomOf(main));
  failing = false;
  await page.getByRole("button", { name: "Try again" }).click();
  await expect(rowAt(page, 1)).toBeVisible();
});

test("the list reads in Dutch", async ({ page }) => {
  await mockSignedIn(page);
  await mockMail(page);
  await mockAccounts(page, ROWS);
  await mockPreferences(page, { locale: "nl" });
  await page.clock.setFixedTime(FIXED_NOW);
  await page.goto("/mail/acc-1/mb-inbox");
  const first = exemplar(0);
  await expect(
    page.getByRole("grid", { name: "Gesprekken" }).locator('[aria-rowindex="1"]'),
  ).toHaveAccessibleName(`${first.from.name}, ${first.subject}, ${FIRST_TIME_NL}, ongelezen`);
  await expect(page.getByRole("main").getByText("23 ongelezen")).toBeAttached();
});

interface Drawn {
  name: string;
  mailboxes: MailboxBody[];
  options: MailOptions;
  offline: boolean;
  settle: (page: Page) => Promise<void>;
}

// The states the sweep draws beside the default list of the shell sweep.
const DRAWN: Drawn[] = [
  {
    name: "first-sync",
    mailboxes: SYNCING,
    options: { vendor: true },
    offline: false,
    settle: (page) => expect(page.getByText("40 of 1,204")).toBeVisible(),
  },
  {
    name: "offline",
    mailboxes: MAILBOXES,
    options: {},
    offline: true,
    settle: (page) => expect(page.getByText(/^Offline: showing cached mail/)).toBeVisible(),
  },
];

test("the list states are axe-clean and match their screenshots at two widths", async ({
  page,
  context,
}) => {
  for (const viewport of VIEWPORTS) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    for (const theme of THEMES) {
      for (const drawn of DRAWN) {
        await test.step(`${drawn.name} in ${theme} at ${viewport.name} width`, async () => {
          await page.emulateMedia({ colorScheme: theme });
          await context.setOffline(false);
          await openInbox(page, drawn.mailboxes, drawn.options);
          await expect(rowAt(page, 1)).toBeVisible();
          await context.setOffline(drawn.offline);
          await drawn.settle(page);
          await page.evaluate(async () => {
            await document.fonts.ready;
          });
          const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
          expect.soft(results.violations, `axe on ${drawn.name} in ${theme}`).toEqual([]);
          await expect
            .soft(page)
            .toHaveScreenshot(`list-${drawn.name}-${theme}-${viewport.name}.png`);
        });
      }
    }
  }
});

test("the failed and the loading list match their screenshots", async ({ page }) => {
  await page.setViewportSize(VIEWPORTS[1]);
  let failing = true;
  let holding = false;
  await mockSignedIn(page);
  await mockMail(page);
  await mockAccounts(page, ROWS);
  await refuseListWhile(
    page,
    () => failing,
    () => holding,
  );
  await page.goto("/mail/acc-1/mb-inbox");
  await expect(page.getByRole("alert")).toContainText("Couldn’t load your mail.");
  await expect.soft(page).toHaveScreenshot("list-failed-light-desktop.png");
  failing = false;
  holding = true;
  await page.getByRole("button", { name: "Try again" }).click();
  const loading = page.getByRole("status", { name: "Loading…" });
  await expect(loading).toBeVisible();
  // The pointer rests where Try again stood; a still row under it takes no hover.
  const still = loading.locator(':scope > [aria-hidden="true"]').nth(2);
  await still.hover();
  expect(await still.evaluate((row) => getComputedStyle(row).backgroundColor)).toBe(TRANSPARENT);
  await page.mouse.move(RESTING_POINT.x, RESTING_POINT.y);
  await expect.soft(page).toHaveScreenshot("list-loading-light-desktop.png");
});

test("the sender starts at one place whether the row carries the dot or the check", async ({
  page,
}) => {
  await page.goto(
    `${STORYBOOK_URL}/iframe.html?id=mail-threadlist--row-states&globals=theme:light`,
  );
  const drawn = page.getByRole("grid", { name: "Row states" }).getByRole("row");
  const plain = drawn.nth(DEFAULT_ROW).getByText("Tomas Lindqvist");
  const picked = drawn.nth(MULTI_SELECTED_ROW).getByText("Tomas Lindqvist");
  await expect(picked).toBeVisible();
  expect((await picked.boundingBox())?.x).toBe((await plain.boundingBox())?.x);
});
