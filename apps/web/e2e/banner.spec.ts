// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { AxeBuilder } from "@axe-core/playwright";
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { accountRow, mockAccounts } from "./account-mocks";
import type { AccountsAnswers, Recorded } from "./account-mocks";
import { FIXED_NOW } from "./mail-corpus";
import { mockMail } from "./mail-mocks";
import { mockPreferences } from "./preference-mocks";
import { mockSignedIn } from "./session-mocks";
import { THEMES, VIEWPORTS, WCAG_TAGS } from "./sweep";

const HOUR_MS = 3_600_000;
// How long the menu's chunk is held back, the way a slow network holds it.
const CHUNK_DELAY_MS = 1_500;
const STOPPED_SENTENCE =
  "Couldn’t reach the server and stopped trying. The connection is checked again every 15 minutes.";
const STILL_STOPPED_SENTENCE =
  "Still couldn’t reach the server. The connection is checked again every 15 minutes.";
const EXPIRED_SENTENCE =
  "The connection to s.bakker@gmail.com expired. Mail shown may be out of date.";

const CONNECTED = accountRow(FIXED_NOW);
const EXPIRED = accountRow(FIXED_NOW, {
  id: "acc-2",
  address: "s.bakker@gmail.com",
  name: "Gmail",
  provider: "gmail",
  kind: "imap",
  authMethod: "password",
  stoppedCause: "credentials",
  stoppedAt: FIXED_NOW.getTime() - HOUR_MS / 2,
  createdAt: FIXED_NOW.getTime() - HOUR_MS / 2,
});
const STOPPED = accountRow(FIXED_NOW, {
  id: "acc-3",
  address: "sanne@dekker-mail.nl",
  name: "dekker-mail.nl",
  provider: "generic",
  kind: "imap",
  authMethod: "password",
  stoppedCause: "connection",
  stoppedAt: FIXED_NOW.getTime() - HOUR_MS / 3,
  createdAt: FIXED_NOW.getTime() - HOUR_MS / 3,
});
const ROWS = [CONNECTED, EXPIRED, STOPPED];

// The mail screen of one account with the three rows behind it.
async function openMail(
  page: Page,
  accountId: string,
  answers: AccountsAnswers = {},
): Promise<Recorded> {
  await mockSignedIn(page);
  await mockMail(page);
  const recorded = await mockAccounts(page, ROWS, answers);
  await page.goto(`/mail/${accountId}`);
  await expect(page.getByRole("heading", { level: 1, name: "Inbox" })).toBeVisible();
  return recorded;
}

function rowAt(page: Page, index: number): Locator {
  return page.getByRole("grid").locator(`[aria-rowindex="${String(index)}"]`);
}

// The account card at the top of the sidebar.
function card(page: Page, address: string): Locator {
  return page.getByRole("button", { name: address });
}

test("an expired account shows the banner with Reconnect to the card, whose pass returns to the mail", async ({
  page,
}) => {
  const { credentials } = await openMail(page, EXPIRED.id);
  const sentence = page.getByRole("status").filter({ hasText: EXPIRED_SENTENCE });
  await expect(sentence).toBeVisible();
  await expect(card(page, EXPIRED.address)).toContainText("Expired");
  const reconnect = page.getByRole("link", { name: "Reconnect Gmail" });
  await expect(reconnect).toHaveAttribute("href", "/accounts/new?reconnect=acc-2");
  await expect(rowAt(page, 1)).toBeVisible();
  await reconnect.click();
  await expect(page).toHaveURL(/\/accounts\/new\?reconnect=acc-2$/);
  await expect(page.getByRole("heading", { level: 1, name: "Reconnect Gmail" })).toBeVisible();
  await page.getByLabel("App password", { exact: true }).fill("abcd efgh ijkl mnop");
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByText("Gmail connected.")).toBeVisible();
  await expect(page).toHaveURL(/\/mail\/acc-2\/mb-inbox$/);
  await expect(sentence).toBeHidden();
  await expect(card(page, EXPIRED.address)).not.toContainText("Expired");
  expect(credentials).toEqual([
    { id: "acc-2", credential: { kind: "password", password: "abcd efgh ijkl mnop" } },
  ]);
});

test("a retry that passes removes the banner, says so and hands the focus to the first row", async ({
  page,
}) => {
  const { retries } = await openMail(page, STOPPED.id);
  const sentence = page.getByRole("status").filter({ hasText: STOPPED_SENTENCE });
  await expect(sentence).toBeVisible();
  await expect(card(page, STOPPED.address)).toContainText("Stopped");
  await expect(rowAt(page, 1)).toBeVisible();
  await page.getByRole("button", { name: "Retry dekker-mail.nl" }).click();
  await expect(page.getByRole("button", { name: /^Retry/ })).toHaveCount(0);
  await expect(page.getByRole("status").filter({ hasText: "Connected again." })).toBeAttached();
  await expect(rowAt(page, 1)).toBeFocused();
  await expect(card(page, STOPPED.address)).not.toContainText("Stopped");
  expect(retries).toEqual(["acc-3"]);
});

test("a retry that still fails says so in place and keeps the cursor; a rejected credential turns Retry into Reconnect", async ({
  page,
}) => {
  await openMail(page, STOPPED.id, {
    retry: [
      { status: 409, cause: "connection" },
      { status: 409, cause: "credentials" },
    ],
  });
  const retry = page.getByRole("button", { name: "Retry dekker-mail.nl" });
  await retry.click();
  await expect(page.getByRole("status").filter({ hasText: STILL_STOPPED_SENTENCE })).toBeVisible();
  await expect(retry).toBeFocused();
  await retry.click();
  const reconnect = page.getByRole("link", { name: "Reconnect dekker-mail.nl" });
  await expect(reconnect).toBeVisible();
  await expect(reconnect).toBeFocused();
  await expect(
    page.getByRole("status").filter({ hasText: "The connection to sanne@dekker-mail.nl expired." }),
  ).toBeVisible();
  await expect(retry).toHaveCount(0);
  await expect(card(page, STOPPED.address)).toContainText("Expired");
});

test("a retry that settled nothing is named in place, whatever refused it", async ({ page }) => {
  await openMail(page, STOPPED.id, { retry: [{ status: 500 }] });
  await page.getByRole("button", { name: "Retry dekker-mail.nl" }).click();
  await expect(
    page.getByRole("status").filter({ hasText: "Couldn’t check the connection." }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: "Retry dekker-mail.nl" })).toBeVisible();
});

test("offline takes the slot from the banner, which returns once the device is back", async ({
  page,
  context,
}) => {
  await openMail(page, STOPPED.id);
  const sentence = page.getByRole("status").filter({ hasText: STOPPED_SENTENCE });
  await expect(sentence).toBeVisible();
  await context.setOffline(true);
  await expect(
    page.getByRole("status").filter({ hasText: "Offline: showing cached mail." }),
  ).toBeVisible();
  await expect(sentence).toBeHidden();
  await expect(page.getByRole("button", { name: /^Retry/ })).toHaveCount(0);
  await context.setOffline(false);
  await expect(sentence).toBeVisible();
  await expect(page.getByRole("button", { name: "Retry dekker-mail.nl" })).toBeVisible();
});

test("a press on the card while the menu's code is still on the way opens the menu once it lands", async ({
  page,
}) => {
  await page.route(/\/assets\/account-menu-[^/]*\.js$/, async (route) => {
    await new Promise((resolve) => {
      setTimeout(resolve, CHUNK_DELAY_MS);
    });
    await route.continue();
  });
  await openMail(page, CONNECTED.id);
  const trigger = card(page, CONNECTED.address);
  await expect(trigger).toHaveAttribute("aria-haspopup", "menu");
  await trigger.click();
  await expect(page.getByRole("menu")).toHaveCount(0);
  await expect(page.getByRole("menu")).toBeVisible();
  await expect(page.getByRole("menuitemradio")).toHaveCount(ROWS.length);
  await expect(trigger).toHaveAttribute("aria-expanded", "true");
});

test("the focus on the card survives the landing of the menu's chunk", async ({ page }) => {
  await page.route(/\/assets\/account-menu-[^/]*\.js$/, async (route) => {
    await new Promise((resolve) => {
      setTimeout(resolve, CHUNK_DELAY_MS);
    });
    await route.continue();
  });
  await openMail(page, CONNECTED.id);
  const trigger = card(page, CONNECTED.address);
  await trigger.focus();
  // The stand-in holds the focus; the landed trigger carries the id the
  // menu is anchored to and takes the focus over.
  await expect(page.locator('nav button[aria-haspopup="menu"]:not([id])')).toBeFocused();
  await expect(page.locator('nav button[aria-haspopup="menu"][id]')).toBeVisible();
  await expect(trigger).toBeFocused();
  await expect(page.getByRole("menu")).toHaveCount(0);
});

test("the account menu shows each inbox's unread count and the mark words of the stopped accounts", async ({
  page,
}) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await openMail(page, CONNECTED.id);
  await card(page, CONNECTED.address).click();
  const menu = page.getByRole("menu");
  await expect(menu.getByRole("menuitemradio")).toHaveText([
    /Fastmail.*sanne@fastmail\.com/,
    /Gmail.*Expired.*s\.bakker@gmail\.com/,
    /dekker-mail\.nl.*Stopped.*sanne@dekker-mail\.nl/,
  ]);
  await expect(menu.getByText("23 unread")).toHaveCount(ROWS.length);
  await expect.soft(page).toHaveScreenshot("banner-menu-light-desktop.png");
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  // On a phone the card sits in the sheet; the menu opens over it.
  await page.setViewportSize(VIEWPORTS[0]);
  await page.getByRole("button", { name: "Mailboxes and accounts" }).click();
  const sheet = page.getByRole("dialog", { name: "Mailboxes and accounts" });
  // The panel has no room to scroll, so a focus inside it moves nothing.
  const close = sheet.getByRole("button", { name: "Close" });
  const closeTop = (await close.boundingBox())?.y;
  expect(closeTop).toBeDefined();
  await sheet.getByRole("treeitem").last().focus();
  expect(await sheet.evaluate((panel) => panel.scrollHeight - panel.clientHeight)).toBe(0);
  expect((await close.boundingBox())?.y).toBe(closeTop);
  await sheet.getByRole("button", { name: CONNECTED.address }).click();
  await expect(menu.getByText("23 unread")).toHaveCount(ROWS.length);
  await expect.soft(page).toHaveScreenshot("banner-menu-light-phone.png");
  await menu.getByRole("menuitemradio", { name: /Gmail/ }).click();
  await expect(page).toHaveURL(/\/mail\/acc-2\/mb-inbox$/);
  await expect(sheet).toBeHidden();
  await expect(page.getByRole("link", { name: "Reconnect Gmail" })).toBeVisible();
});

test("the banner, the marks and the badges read in Dutch and in the pseudo-locale", async ({
  page,
}) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await mockSignedIn(page);
  await mockMail(page);
  await mockAccounts(page, ROWS);
  await mockPreferences(page, { locale: "nl" });
  await page.goto(`/mail/${EXPIRED.id}`);
  await expect(
    page.getByText(
      "De verbinding met s.bakker@gmail.com is verlopen. De getoonde mail is misschien niet actueel.",
    ),
  ).toBeVisible();
  await expect(page.getByRole("link", { name: "Gmail opnieuw verbinden" })).toBeVisible();
  await expect(card(page, EXPIRED.address)).toContainText("Verlopen");
  await expect.soft(page).toHaveScreenshot("banner-nl-light-desktop.png");
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "en-XA");
  });
  await page.goto(`/mail/${EXPIRED.id}`);
  await expect(page.getByRole("link", { name: /Réçóññéçt/ })).toBeVisible();
  await expect(card(page, EXPIRED.address)).toContainText("Éxpíréd");
  await expect.soft(page).toHaveScreenshot("banner-expired-en-XA-light-desktop.png");
  await page.goto(`/mail/${STOPPED.id}`);
  await expect(page.getByRole("button", { name: /Rétrý/ })).toBeVisible();
  await card(page, STOPPED.address).click();
  await expect(page.getByRole("menu").getByText(/úñréád/)).toHaveCount(ROWS.length);
  await expect.soft(page).toHaveScreenshot("banner-menu-en-XA-light-desktop.png");
});

// The two causes the sweep draws over the list.
const DRAWN = [
  { name: "expired", accountId: EXPIRED.id, sentence: EXPIRED_SENTENCE },
  { name: "stopped", accountId: STOPPED.id, sentence: STOPPED_SENTENCE },
];

test("the banner is axe-clean and matches its screenshots at two widths", async ({ page }) => {
  for (const viewport of VIEWPORTS) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    for (const theme of THEMES) {
      for (const drawn of DRAWN) {
        await test.step(`${drawn.name} in ${theme} at ${viewport.name} width`, async () => {
          await page.emulateMedia({ colorScheme: theme });
          await openMail(page, drawn.accountId);
          await expect(page.getByRole("status").filter({ hasText: drawn.sentence })).toBeVisible();
          await expect(rowAt(page, 1)).toBeVisible();
          await page.evaluate(async () => {
            await document.fonts.ready;
          });
          const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
          expect.soft(results.violations, `axe on ${drawn.name} in ${theme}`).toEqual([]);
          await expect
            .soft(page)
            .toHaveScreenshot(`banner-${drawn.name}-${theme}-${viewport.name}.png`);
        });
      }
    }
  }
});
