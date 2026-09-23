// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { AxeBuilder } from "@axe-core/playwright";
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { FIXED_NOW } from "./account-card";
import { accountRow, mockAccounts } from "./account-mocks";
import type { AccountRowBody, AccountsAnswers, Recorded } from "./account-mocks";
import { mockPreferences } from "./preference-mocks";
import { mockSignedIn } from "./session-mocks";
import { THEMES, VIEWPORTS, WCAG_TAGS } from "./sweep";

// The undo window plus a margin, so an expired timer has certainly fired.
const UNDO_WINDOW_MS = 5_000;
const PAST_WINDOW_MS = UNDO_WINDOW_MS + 1_000;
const LIST_DELAY_MS = 1_500;
const HOUR_MS = 3_600_000;
const STOPPED_SENTENCE =
  "Couldn’t reach the server and stopped trying. The connection is checked again every 15 minutes.";

const CONNECTED = accountRow(FIXED_NOW);
const EXPIRED = accountRow(FIXED_NOW, {
  id: "acc-2",
  address: "s.bakker@gmail.com",
  name: "Gmail",
  provider: "gmail",
  kind: "imap",
  authMethod: "password",
  stoppedCause: "credentials",
  stoppedAt: FIXED_NOW.getTime() - HOUR_MS,
});
const STOPPED = accountRow(FIXED_NOW, {
  id: "acc-3",
  address: "sanne@dekker-mail.nl",
  name: "dekker-mail.nl",
  provider: "generic",
  kind: "imap",
  authMethod: "password",
  stoppedCause: "connection",
  stoppedAt: FIXED_NOW.getTime() - HOUR_MS,
});
const ROWS = [CONNECTED, EXPIRED, STOPPED];

async function openAccounts(
  page: Page,
  rows: AccountRowBody[] = ROWS,
  answers: AccountsAnswers = {},
): Promise<Recorded> {
  await mockSignedIn(page);
  const recorded = await mockAccounts(page, rows, answers);
  await page.clock.install({ time: FIXED_NOW });
  await page.goto("/settings/accounts");
  await expect(page.getByRole("heading", { level: 2, name: "Mail accounts" })).toBeVisible();
  return recorded;
}

function rowOf(page: Page, address: string): Locator {
  return page.getByRole("listitem").filter({ hasText: address });
}

test("the list names each account, its state and the actions it has", async ({ page }) => {
  await openAccounts(page);
  await expect(page.getByRole("list", { name: "Mail accounts" }).getByRole("listitem")).toHaveCount(
    3,
  );
  const connected = rowOf(page, CONNECTED.address);
  await expect(connected).toContainText("Fastmail");
  await expect(connected.locator('[aria-hidden="true"]').first()).toHaveText("F");
  await expect(connected.getByRole("button", { name: "Remove Fastmail" })).toBeVisible();
  await expect(connected.getByRole("button")).toHaveCount(1);
  await expect(connected.getByRole("link")).toHaveCount(0);
  const expired = rowOf(page, EXPIRED.address);
  await expect(expired).toContainText("Connection expired");
  await expect(expired.getByRole("link", { name: "Reconnect Gmail" })).toHaveAttribute(
    "href",
    "/accounts/new?reconnect=acc-2",
  );
  const stopped = rowOf(page, STOPPED.address);
  await expect(stopped).toContainText(STOPPED_SENTENCE);
  await expect(stopped.getByRole("button", { name: "Retry dekker-mail.nl" })).toBeVisible();
  await expect(page.getByRole("link", { name: "Add account" })).toHaveAttribute(
    "href",
    "/accounts/new",
  );
});

test("removing an account waits behind the toast; Undo and the z key put it back", async ({
  page,
}) => {
  const { deletes } = await openAccounts(page);
  await page.getByRole("button", { name: "Remove Gmail" }).click();
  await expect(rowOf(page, EXPIRED.address)).toBeHidden();
  await expect(page.getByText("Gmail removed.")).toBeVisible();
  await page.getByRole("button", { name: "Undo" }).click();
  await expect(rowOf(page, EXPIRED.address)).toBeVisible();
  await page.getByRole("button", { name: "Remove Fastmail" }).click();
  await expect(page.getByText("Fastmail removed.")).toBeVisible();
  await page.keyboard.press("z");
  await expect(rowOf(page, CONNECTED.address)).toBeVisible();
  await page.clock.runFor(PAST_WINDOW_MS);
  expect(deletes).toEqual([]);
});

test("when the toast runs out the server hears about it and the row stays gone", async ({
  page,
}) => {
  const { deletes } = await openAccounts(page);
  await page.getByRole("button", { name: "Remove Gmail" }).click();
  await page.clock.runFor(PAST_WINDOW_MS);
  await expect(page.getByText("Gmail removed.")).toBeHidden();
  await expect.poll(() => deletes).toEqual(["acc-2"]);
  await expect(rowOf(page, EXPIRED.address)).toBeHidden();
});

test("a keyboard remove hands focus to a neighboring row, then to Add account", async ({
  page,
}) => {
  await openAccounts(page);
  await page.getByRole("button", { name: "Remove Gmail" }).focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("button", { name: "Remove dekker-mail.nl" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("button", { name: "Remove Fastmail" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("link", { name: "Add account" })).toBeFocused();
  await expect(page.getByText("Your mail stays at your provider.")).toBeVisible();
  await expect(page.getByRole("list", { name: "Mail accounts" })).toHaveCount(0);
});

test("a retry that passes clears the state line, says so and hands the cursor to the row", async ({
  page,
}) => {
  const { retries } = await openAccounts(page);
  const stopped = rowOf(page, STOPPED.address);
  await stopped.getByRole("button", { name: "Retry dekker-mail.nl" }).focus();
  await page.keyboard.press("Enter");
  await expect(stopped.getByRole("status")).toHaveText("Connected again.");
  await expect(stopped).not.toContainText("stopped trying");
  await expect(stopped.getByRole("button", { name: /^Retry/ })).toHaveCount(0);
  await expect(stopped).toBeFocused();
  expect(retries).toEqual(["acc-3"]);
});

test("a retry that still fails says so inline; a rejected credential turns Retry into Reconnect", async ({
  page,
}) => {
  await openAccounts(page, ROWS, {
    retry: [
      { status: 409, cause: "connection" },
      { status: 409, cause: "credentials" },
    ],
  });
  const stopped = rowOf(page, STOPPED.address);
  const retry = stopped.getByRole("button", { name: "Retry dekker-mail.nl" });
  await retry.click();
  await expect(stopped.getByRole("alert")).toContainText("Still couldn’t reach the server");
  await expect(retry).toBeFocused();
  await retry.click();
  await expect(stopped).toContainText("Connection expired");
  await expect(stopped.getByRole("link", { name: "Reconnect dekker-mail.nl" })).toBeVisible();
  await expect(retry).toHaveCount(0);
  await expect(stopped).toBeFocused();
});

test("a retry that settled nothing is named on the row, whatever refused it", async ({ page }) => {
  await openAccounts(page, ROWS, {
    retry: [{ status: 500 }, { status: 502, error: "upstream_unreachable" }],
  });
  const stopped = rowOf(page, STOPPED.address);
  await stopped.getByRole("button", { name: "Retry dekker-mail.nl" }).click();
  await expect(stopped.getByRole("alert")).toContainText("Couldn’t check the connection");
  await expect(stopped).toContainText("Retry");
  await stopped.getByRole("button", { name: "Retry dekker-mail.nl" }).click();
  await expect(stopped.getByRole("alert")).toContainText("Couldn’t check the connection");
  await expect(stopped).toContainText("Retry");
});

test("a retry on a row the server dropped leaves the list with the next answer", async ({
  page,
}) => {
  const { retries } = await openAccounts(page, ROWS, {
    retry: [{ status: 404, error: "not_found" }],
  });
  await rowOf(page, STOPPED.address).getByRole("button", { name: "Retry dekker-mail.nl" }).click();
  await expect(rowOf(page, STOPPED.address)).toBeHidden();
  await expect(page.getByRole("list", { name: "Mail accounts" }).getByRole("listitem")).toHaveCount(
    2,
  );
  expect(retries).toEqual(["acc-3"]);
});

test("a removal forgets the row's retry outcome, so Undo brings it back without the old alert", async ({
  page,
}) => {
  await openAccounts(page, ROWS, { retry: [{ status: 409, cause: "connection" }] });
  const stopped = rowOf(page, STOPPED.address);
  await stopped.getByRole("button", { name: "Retry dekker-mail.nl" }).click();
  await expect(stopped.getByRole("alert")).toContainText("Still couldn’t reach the server");
  await page.getByRole("button", { name: "Remove dekker-mail.nl" }).click();
  await page.getByRole("button", { name: "Undo" }).click();
  await expect(stopped).toContainText(STOPPED_SENTENCE);
  await expect(stopped.getByRole("alert")).toHaveCount(0);
});

test("a removal the server refuses brings the row back and says so", async ({ page }) => {
  const { deletes } = await openAccounts(page, ROWS, { remove: [{ status: 500 }] });
  await page.getByRole("button", { name: "Remove Gmail" }).click();
  await page.clock.runFor(PAST_WINDOW_MS);
  await expect(page.getByText("Couldn’t remove that account.")).toBeVisible();
  await expect(rowOf(page, EXPIRED.address)).toBeVisible();
  expect(deletes).toEqual(["acc-2"]);
});

test("Reconnect opens the card on the row and a passing credential lands back on the page", async ({
  page,
}) => {
  const { credentials } = await openAccounts(page);
  await page.getByRole("link", { name: "Reconnect Gmail" }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Reconnect Gmail" })).toBeVisible();
  await page.getByLabel("App password", { exact: true }).fill("abcd efgh ijkl mnop");
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page).toHaveURL(/\/settings\/accounts$/);
  await expect(page.getByText("Gmail connected.")).toBeVisible();
  await expect(rowOf(page, EXPIRED.address)).not.toContainText("Connection expired");
  expect(credentials).toEqual([
    { id: "acc-2", credential: { kind: "password", password: "abcd efgh ijkl mnop" } },
  ]);
});

test("without accounts the page invites and Add account opens the card", async ({ page }) => {
  await openAccounts(page, []);
  await expect(page.getByText("Your mail stays at your provider.")).toBeVisible();
  await expect(page.getByRole("list")).toHaveCount(0);
  await page.getByRole("link", { name: "Add account" }).click();
  await expect(page).toHaveURL(/\/accounts\/new$/);
  await expect(page.getByRole("heading", { level: 1, name: "Add a mail account" })).toBeVisible();
});

test("a slow list shows skeleton rows first", async ({ page }) => {
  await mockSignedIn(page);
  await page.route("**/api/accounts", async (route) => {
    if (route.request().method() !== "GET") {
      return route.fallback();
    }
    await new Promise((resolve) => {
      setTimeout(resolve, LIST_DELAY_MS);
    });
    return route.fulfill({ json: { accounts: ROWS, probeIntervalMinutes: 15 } });
  });
  await page.goto("/settings/accounts");
  await expect(page.getByLabel("Loading…")).toBeVisible();
  await expect(rowOf(page, CONNECTED.address)).toBeVisible();
});

test("a failed list offers a retry that works", async ({ page }) => {
  await mockSignedIn(page);
  await mockAccounts(page, ROWS, { list: [500] });
  await page.goto("/settings/accounts");
  await expect(page.getByRole("alert")).toContainText("Couldn’t load your accounts.");
  await page.getByRole("button", { name: "Try again" }).click();
  await expect(rowOf(page, CONNECTED.address)).toBeVisible();
});

test("a session that ended meanwhile signs out and says so", async ({ page }) => {
  await openAccounts(page, ROWS, { retry: [{ status: 401, error: "unauthenticated" }] });
  await page.getByRole("button", { name: "Retry dekker-mail.nl" }).click();
  await expect(page).toHaveURL(/\/sign-in$/);
  await expect(page.getByText("Your session has ended. Sign in again.")).toBeVisible();
});

test("the page is axe-clean and matches its screenshots", async ({ page }) => {
  for (const viewport of VIEWPORTS) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    for (const theme of THEMES) {
      await test.step(`in ${theme} at ${viewport.name} width`, async () => {
        await page.emulateMedia({ colorScheme: theme });
        await openAccounts(page);
        await page.evaluate(async () => {
          await document.fonts.ready;
        });
        const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
        expect.soft(results.violations, `axe in ${theme} at ${viewport.name} width`).toEqual([]);
        await expect.soft(page).toHaveScreenshot(`accounts-${theme}-${viewport.name}.png`, {
          fullPage: true,
        });
      });
    }
  }
});

test("the page reads in Dutch and in the pseudo-locale", async ({ page }) => {
  const desktop = VIEWPORTS[1];
  await page.setViewportSize({ width: desktop.width, height: desktop.height });
  await mockSignedIn(page);
  await mockAccounts(page, ROWS);
  await page.clock.install({ time: FIXED_NOW });
  await mockPreferences(page, { locale: "nl" });
  await page.goto("/settings/accounts");
  await expect(page.getByText("Verbinding verlopen")).toBeVisible();
  await expect(page.getByText("elke 15 minuten")).toBeVisible();
  await expect(page.getByRole("link", { name: "Account toevoegen" })).toBeVisible();
  await expect.soft(page).toHaveScreenshot("accounts-nl-light-desktop.png", { fullPage: true });
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "en-XA");
  });
  await page.goto("/settings/accounts");
  await expect(page.getByRole("heading", { level: 2, name: /Máíl áççóúñts/ })).toBeVisible();
  await expect.soft(page).toHaveScreenshot("accounts-en-XA-light-desktop.png", { fullPage: true });
});
