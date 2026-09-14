// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { FIXED_NOW, card, field, openCard, stopAt, typeInto } from "./account-card";
import type { Walk } from "./account-card";
import { GMAIL_FOUND, accountRow, mockAccounts, mockConsent } from "./account-mocks";
import type { FoundBody } from "./account-mocks";
import { mockSignedIn } from "./session-mocks";
import type { MockSignIn } from "./session-mocks";

const GMAIL_ADDRESS = "sanne@gmail.com";
const GMAIL_SIGN_IN: FoundBody = { ...GMAIL_FOUND, oauthAvailable: true };
const MICROSOFT_SIGN_IN: FoundBody = {
  status: "found",
  provider: "microsoft",
  kind: "imap",
  target: {
    kind: "imap",
    username: "sanne@outlook.com",
    imap: { host: "outlook.office365.com", port: 993, tls: "implicit" },
    smtp: { host: "smtp.office365.com", port: 587, tls: "starttls" },
  },
  credentialKind: "oauth",
  host: "outlook.office365.com",
  oauthAvailable: true,
};
const BOTH: MockSignIn[] = ["google", "microsoft"];
const PROVIDER_PAGE = /accounts\.google\.test/;
// Longer than one poll interval, so a poll that kept going would show.
const POLL_SETTLE_MS = 2_500;

// The provider's window, as the click opens it.
async function clickForWindow(page: Page, name: string): Promise<Page> {
  const opened = page.waitForEvent("popup");
  await page.getByRole("button", { name }).click();
  return opened;
}

test("a Gmail address by consent takes three stops and one typed field; the window holds no opener", async ({
  page,
}) => {
  const { adds } = await openCard(
    page,
    { discover: [GMAIL_SIGN_IN] },
    [accountRow(FIXED_NOW)],
    BOTH,
  );
  const consent = await mockConsent(page, {
    outcomes: [{ status: "pending" }, { status: "done", accountId: "acc-1" }],
  });
  const walk: Walk = { stops: [], typed: [] };
  await stopAt(page, walk, "typing");
  await typeInto(page, walk, "Email address", GMAIL_ADDRESS);
  await page.getByRole("button", { name: "Continue", exact: true }).click();
  await stopAt(page, walk, "found");
  const popup = await clickForWindow(page, "Continue with Google");
  await stopAt(page, walk, "consent");
  await expect(page.getByRole("status")).toContainText("A Google window is open");
  await popup.waitForURL(PROVIDER_PAGE);
  expect(await popup.evaluate(() => window.opener === null)).toBe(true);
  await expect(page.getByText("Gmail connected.")).toBeVisible();
  await expect(page).toHaveURL(/\/$/);
  expect(walk.stops).toEqual(["typing", "found", "consent"]);
  expect(walk.typed).toEqual(["Email address"]);
  expect(consent.starts).toEqual([{ provider: "gmail", address: GMAIL_ADDRESS }]);
  expect(consent.polls).toBeGreaterThanOrEqual(2);
  expect(adds).toEqual([]);
});

test("the first screen starts a consent from its buttons; a denied one offers the password route into discovery", async ({
  page,
}) => {
  await openCard(page, { discover: [GMAIL_FOUND] }, [], BOTH);
  const consent = await mockConsent(page, {
    outcomes: [{ status: "denied", cause: "accessDenied" }],
  });
  await expect(page.getByText("Google and Microsoft sign-in are not set up")).toHaveCount(0);
  await field(page, "Email address").fill(GMAIL_ADDRESS);
  await clickForWindow(page, "Continue with Google");
  await expect(card(page)).toHaveAttribute("data-step", "consentDenied");
  await expect(page.getByRole("alert")).toContainText("Google didn’t grant access");
  await expect(page.getByRole("button", { name: "Try again" })).toBeFocused();
  await page.getByRole("button", { name: "Use a password instead" }).click();
  await expect(card(page)).toHaveAttribute("data-step", "found");
  await expect(field(page, "App password")).toBeFocused();
  expect(consent.starts).toEqual([{ provider: "gmail", address: GMAIL_ADDRESS }]);
});

test("Try again starts a fresh consent; a consent that ran out says so and Microsoft offers no password", async ({
  page,
}) => {
  await openCard(page, { discover: [MICROSOFT_SIGN_IN] }, [], BOTH);
  const consent = await mockConsent(page, { outcomes: [{ status: 404 }] });
  await field(page, "Email address").fill("sanne@outlook.com");
  await page.getByRole("button", { name: "Continue", exact: true }).click();
  await expect(card(page)).toHaveAttribute("data-step", "found");
  await expect(page.getByText(/the admin does/)).toHaveCount(0);
  await clickForWindow(page, "Continue with Microsoft");
  await expect(page.getByRole("alert")).toContainText("wasn’t finished in time");
  await expect(page.getByRole("button", { name: "Use a password instead" })).toHaveCount(0);
  await clickForWindow(page, "Try again");
  await expect(card(page)).toHaveAttribute("data-step", "consentDenied");
  expect(consent.starts).toHaveLength(2);
});

// Where a consent starts from and which control takes the cursor back.
const CANCEL_ORIGINS = [
  {
    name: "Google",
    found: GMAIL_SIGN_IN,
    address: GMAIL_ADDRESS,
    control: (page: Page) => field(page, "App password"),
  },
  {
    name: "Microsoft",
    found: MICROSOFT_SIGN_IN,
    address: "sanne@outlook.com",
    control: (page: Page) => page.getByRole("button", { name: "Continue with Microsoft" }),
  },
];

for (const origin of CANCEL_ORIGINS) {
  test(`Cancel returns to the ${origin.name} step it left, keeps the address and focuses its first control`, async ({
    page,
  }) => {
    await openCard(page, { discover: [origin.found] }, [], BOTH);
    const consent = await mockConsent(page, { outcomes: [{ status: "pending" }] });
    await field(page, "Email address").fill(origin.address);
    await page.getByRole("button", { name: "Continue", exact: true }).click();
    const popup = await clickForWindow(page, `Continue with ${origin.name}`);
    await popup.waitForURL(PROVIDER_PAGE);
    await expect(page.getByRole("button", { name: "Cancel" })).toBeFocused();
    await page.getByRole("button", { name: "Cancel" }).click();
    await expect(card(page)).toHaveAttribute("data-step", "found");
    await expect(field(page, "Email address")).toHaveValue(origin.address);
    await expect(origin.control(page)).toBeFocused();
    const polled = consent.polls;
    await page.waitForTimeout(POLL_SETTLE_MS);
    expect(consent.polls).toBe(polled);
  });
}

test("a session that ends mid-consent signs out and says so", async ({ page }) => {
  await openCard(page, { discover: [GMAIL_SIGN_IN] }, [], BOTH);
  await mockConsent(page, {
    outcomes: [{ status: "pending" }, { status: 401, error: "unauthenticated" }],
  });
  await field(page, "Email address").fill(GMAIL_ADDRESS);
  await page.getByRole("button", { name: "Continue", exact: true }).click();
  await clickForWindow(page, "Continue with Google");
  await expect(page).toHaveURL(/\/sign-in$/);
  await expect(page.getByText("Your session has ended. Sign in again.")).toBeVisible();
});

test("an OAuth row reconnects through its consent and the tokens land on the row", async ({
  page,
}) => {
  await mockSignedIn(page, "owner", ["google"]);
  const stopped = accountRow(FIXED_NOW, {
    address: GMAIL_ADDRESS,
    name: "Gmail",
    provider: "gmail",
    kind: "imap",
    authMethod: "oauth2",
    stoppedCause: "credentials",
    stoppedAt: FIXED_NOW.getTime(),
  });
  await mockAccounts(page, [stopped]);
  const consent = await mockConsent(page, {
    outcomes: [{ status: "done", accountId: "acc-1" }],
  });
  await page.goto("/accounts/new?reconnect=acc-1");
  await expect(page.getByRole("heading", { level: 1, name: "Reconnect Gmail" })).toBeVisible();
  await expect(page.getByLabel(/password/i)).toHaveCount(0);
  await clickForWindow(page, "Continue with Google");
  await expect(page.getByText("Gmail connected.")).toBeVisible();
  expect(consent.starts).toEqual([
    { provider: "gmail", address: GMAIL_ADDRESS, accountId: "acc-1" },
  ]);
});

test("an instance that lost its provider says so; a malformed address starts nothing", async ({
  page,
}) => {
  await openCard(page, { discover: [GMAIL_SIGN_IN] }, [], BOTH);
  const consent = await mockConsent(page, {
    start: [{ status: 409, error: "provider_not_configured" }],
    outcomes: [],
  });
  await field(page, "Email address").fill("sanne@localhost");
  await page.getByRole("button", { name: "Continue with Google" }).click();
  await expect(page.getByRole("alert")).toContainText("Enter a mail address");
  expect(consent.starts).toEqual([]);
  await field(page, "Email address").fill(GMAIL_ADDRESS);
  await page.getByRole("button", { name: "Continue", exact: true }).click();
  await expect(card(page)).toHaveAttribute("data-step", "found");
  const popup = await clickForWindow(page, "Continue with Google");
  await expect(card(page)).toHaveAttribute("data-step", "found");
  await expect(page.getByRole("alert")).toContainText("not set up here");
  await expect.poll(() => popup.isClosed()).toBe(true);
  expect(consent.starts).toHaveLength(1);
});
