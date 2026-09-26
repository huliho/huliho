// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { AxeBuilder } from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { FIXED_NOW, card, field, openCard, stopAt, typeInto } from "./account-card";
import type { Walk } from "./account-card";
import {
  DOVECOT_FOUND,
  FASTMAIL_FOUND,
  GMAIL_FOUND,
  accountRow,
  mockAccounts,
} from "./account-mocks";
import { mockMail } from "./mail-mocks";
import { mockPreferences } from "./preference-mocks";
import { mockSignedIn } from "./session-mocks";
import { THEMES, VIEWPORTS, WCAG_TAGS } from "./sweep";

const RETRY_SECONDS = 90;
const FASTMAIL_ADDRESS = "sanne@fastmail.com";
const GENERIC_ADDRESS = "sanne@dekker-mail.nl";
const SERVER = "mail.dekker-mail.nl";
const TOKEN = "fmu1-example-token";
const PASSWORD = "example passphrase";

// The discovered route from the address to Connect; the label says which
// secret the provider takes.
async function walkDiscovered(
  page: Page,
  address: string,
  credentialLabel: string,
  secret: string,
): Promise<Walk> {
  const walk: Walk = { stops: [], typed: [] };
  await stopAt(page, walk, "typing");
  await typeInto(page, walk, "Email address", address);
  await page.getByRole("button", { name: "Continue" }).click();
  await stopAt(page, walk, "found");
  await typeInto(page, walk, credentialLabel, secret);
  await page.getByRole("button", { name: "Continue" }).click();
  await stopAt(page, walk, "confirmHost");
  await page.getByRole("button", { name: "Connect" }).click();
  return walk;
}

// The manual route up to a filled form: nothing found, then the server and the password.
async function fillManual(page: Page): Promise<Walk> {
  const walk: Walk = { stops: [], typed: [] };
  await stopAt(page, walk, "typing");
  await typeInto(page, walk, "Email address", GENERIC_ADDRESS);
  await page.getByRole("button", { name: "Continue" }).click();
  await stopAt(page, walk, "notFound");
  await page.getByRole("button", { name: "Enter server details" }).click();
  await stopAt(page, walk, "manual");
  await expect(field(page, "Server")).toBeFocused();
  await typeInto(page, walk, "Server", SERVER);
  await typeInto(page, walk, "Password", PASSWORD);
  return walk;
}

test("a session without an account lands on the card, with Settings and sign-out in reach", async ({
  page,
}) => {
  await mockSignedIn(page);
  await mockAccounts(page, []);
  await page.goto("/");
  await expect(page).toHaveURL(/\/accounts\/new$/);
  await expect(page.getByRole("heading", { level: 1, name: "Add a mail account" })).toBeVisible();
  await expect(page.getByText("Google and Microsoft sign-in are not set up")).toBeVisible();
  await expect(page.getByRole("link", { name: "Settings" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Sign out" })).toBeVisible();
  await expect(page.getByLabel("Email address")).toBeFocused();
});

test("a Fastmail address takes three stops and two typed fields; the token goes out as a bearer", async ({
  page,
}) => {
  const { adds } = await openCard(page, { discover: [FASTMAIL_FOUND] });
  const walk = await walkDiscovered(page, FASTMAIL_ADDRESS, "API token", TOKEN);
  await expect(page.getByText("Fastmail connected.")).toBeVisible();
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-inbox$/);
  await expect(page.getByRole("heading", { level: 1, name: "Inbox" })).toBeVisible();
  await expect(page.getByRole("button", { name: /sanne@fastmail\.com/ })).toBeVisible();
  expect(walk.stops).toEqual(["typing", "found", "confirmHost"]);
  expect(walk.typed).toHaveLength(2);
  expect(adds).toEqual([
    {
      address: FASTMAIL_ADDRESS,
      provider: "fastmail",
      target: FASTMAIL_FOUND.target,
      credential: { kind: "bearer", token: TOKEN },
    },
  ]);
});

test("the found step names the server and the secret; Change and Use a different server lead away", async ({
  page,
}) => {
  await openCard(page, { discover: [FASTMAIL_FOUND, FASTMAIL_FOUND] });
  await field(page, "Email address").fill(FASTMAIL_ADDRESS);
  await page.getByRole("button", { name: "Continue" }).click();
  await expect(page.getByText("Found Fastmail. Connecting to api.fastmail.com.")).toBeVisible();
  await expect(page.getByText("Fastmail sign-in needs an API token")).toBeVisible();
  await expect(field(page, "API token")).toBeFocused();
  await page.getByRole("button", { name: "Change" }).click();
  await expect(card(page)).toHaveAttribute("data-step", "typing");
  await expect(field(page, "Email address")).toHaveValue(FASTMAIL_ADDRESS);
  await page.getByRole("button", { name: "Continue" }).click();
  await field(page, "API token").fill(TOKEN);
  await page.getByRole("button", { name: "Continue" }).click();
  await expect(page.getByText("Your token is only sent after you confirm.")).toBeVisible();
  await expect(page.getByText("api.fastmail.com", { exact: true })).toBeVisible();
  await expect(page.getByText("Encrypted connection · Fastmail")).toBeVisible();
  await expect(page.getByRole("button", { name: "Connect" })).toBeFocused();
  await page.getByRole("button", { name: "Use a different server" }).click();
  await expect(card(page)).toHaveAttribute("data-step", "manual");
});

test("a Gmail address asks for an app password with its instructions", async ({ page }) => {
  const { adds } = await openCard(page, { discover: [GMAIL_FOUND] });
  const walk = await walkDiscovered(page, "sanne@gmail.com", "App password", "abcd efgh ijkl mnop");
  await expect(page.getByText("Gmail connected.")).toBeVisible();
  expect(walk.stops).toHaveLength(3);
  expect(walk.typed).toHaveLength(2);
  expect(adds.map((add) => add.credential)).toEqual([
    { kind: "password", password: "abcd efgh ijkl mnop" },
  ]);
});

test("a server by password keeps the password typed on the first screen", async ({ page }) => {
  const { adds } = await openCard(page, { discover: [DOVECOT_FOUND] });
  const walk: Walk = { stops: [], typed: [] };
  await stopAt(page, walk, "typing");
  await typeInto(page, walk, "Email address", GENERIC_ADDRESS);
  await typeInto(page, walk, "Password", PASSWORD);
  await page.getByRole("button", { name: "Continue" }).click();
  await stopAt(page, walk, "found");
  await expect(field(page, "Password")).toHaveValue(PASSWORD);
  await page.getByRole("button", { name: "Continue" }).click();
  await stopAt(page, walk, "confirmHost");
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByText("dekker-mail.nl connected.")).toBeVisible();
  expect(walk.stops).toHaveLength(3);
  expect(walk.typed).toHaveLength(2);
  expect(adds.map((add) => add.credential)).toEqual([{ kind: "password", password: PASSWORD }]);
});

test("nothing found offers manual entry; the form follows itself and connects", async ({
  page,
}) => {
  const { adds } = await openCard(page);
  const walk = await fillManual(page);
  await expect(field(page, "Port")).toHaveValue("993");
  await expect(field(page, "Username")).toHaveValue(GENERIC_ADDRESS);
  await page.getByText("Outgoing server").click();
  const outgoing = page.locator("details");
  await expect(outgoing.getByLabel("Server", { exact: true })).toHaveValue(SERVER);
  await expect(outgoing.getByLabel("Port", { exact: true })).toHaveValue("465");
  await outgoing.getByLabel("Encryption").selectOption("starttls");
  await expect(outgoing.getByLabel("Port", { exact: true })).toHaveValue("587");
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByText("dekker-mail.nl connected.")).toBeVisible();
  expect(walk.stops).toEqual(["typing", "notFound", "manual"]);
  expect(walk.typed).toHaveLength(3);
  expect(adds).toEqual([
    {
      address: GENERIC_ADDRESS,
      provider: "generic",
      target: {
        kind: "imap",
        username: GENERIC_ADDRESS,
        imap: { host: SERVER, port: 993, tls: "implicit" },
        smtp: { host: SERVER, port: 587, tls: "starttls" },
      },
      credential: { kind: "password", password: PASSWORD },
    },
  ]);
});

test("a session URL turns the manual form into a JMAP one", async ({ page }) => {
  const { adds } = await openCard(page);
  await fillManual(page);
  await field(page, "Server").fill("https://localhost:8443/jmap");
  await expect(page.getByLabel("Port", { exact: true })).toHaveCount(0);
  await expect(page.getByLabel("Username")).toHaveCount(0);
  await expect(page.getByText("Outgoing server")).toHaveCount(0);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByText("dekker-mail.nl connected.")).toBeVisible();
  expect(adds.map((add) => add.target)).toEqual([
    { kind: "jmap", sessionUrl: "https://localhost:8443/jmap" },
  ]);
});

test("a wrong credential marks the field and the next attempt goes through", async ({ page }) => {
  const { adds } = await openCard(page, {
    discover: [FASTMAIL_FOUND],
    add: [{ status: 401, error: "upstream_credentials" }],
  });
  await walkDiscovered(page, FASTMAIL_ADDRESS, "API token", "wrong token");
  await expect(card(page)).toHaveAttribute("data-step", "found");
  await expect(page.getByRole("alert")).toContainText("Check the address and the token");
  await expect(field(page, "API token")).toHaveAttribute("aria-invalid", "true");
  await expect(field(page, "API token")).toBeFocused();
  await field(page, "API token").fill(TOKEN);
  await expect(page.getByRole("alert")).toHaveCount(0);
  await page.getByRole("button", { name: "Continue" }).click();
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByText("Fastmail connected.")).toBeVisible();
  expect(adds.map((add) => add.credential)).toEqual([
    { kind: "bearer", token: "wrong token" },
    { kind: "bearer", token: TOKEN },
  ]);
});

test("an insecure server gets its own screen and Back keeps what was typed", async ({ page }) => {
  await openCard(page, { add: [{ status: 400, error: "upstream_insecure" }] });
  await fillManual(page);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(card(page)).toHaveAttribute("data-step", "insecure");
  await expect(page.getByRole("alert")).toContainText("only offers an unencrypted connection");
  await expect(page.getByText("Ask whoever runs this mail server")).toBeVisible();
  await expect(page.getByRole("button", { name: "Back" })).toBeFocused();
  await page.getByRole("button", { name: "Back" }).click();
  await expect(card(page)).toHaveAttribute("data-step", "manual");
  await expect(field(page, "Server")).toHaveValue(SERVER);
});

test("an unreachable server and a mailbox without SMTP AUTH are named above the fields", async ({
  page,
}) => {
  await openCard(page, {
    add: [
      { status: 502, error: "upstream_unreachable" },
      { status: 400, error: "smtp_auth_unavailable" },
    ],
  });
  await fillManual(page);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByRole("alert")).toContainText(`Couldn’t reach ${SERVER}`);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByRole("alert")).toContainText("turn on SMTP AUTH");
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByText("dekker-mail.nl connected.")).toBeVisible();
});

test("a malformed address and a bad server name never leave the browser", async ({ page }) => {
  const { discoveries, adds } = await openCard(page);
  await field(page, "Email address").fill("sanne@localhost");
  await page.getByRole("button", { name: "Continue" }).click();
  await expect(page.getByRole("alert")).toContainText("Enter a mail address");
  await expect(field(page, "Email address")).toHaveAttribute("aria-invalid", "true");
  expect(discoveries).toEqual([]);
  await fillManual(page);
  await field(page, "Server").fill("mail.dekker-mail.nl:993");
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByRole("alert")).toContainText("Enter a server name");
  await expect(field(page, "Server")).toBeFocused();
  expect(adds).toEqual([]);
});

test("the limiter holds the form with a countdown", async ({ page }) => {
  await openCard(page, {
    discover: [{ status: 429, error: "rate_limited", retryAfter: RETRY_SECONDS }],
  });
  await field(page, "Email address").fill(FASTMAIL_ADDRESS);
  await page.getByRole("button", { name: "Continue" }).click();
  await expect(page.getByRole("alert")).toContainText("Too many attempts");
  await expect(page.getByRole("button", { name: /^Try again in 0?1:/ })).toBeVisible();
  await expect(field(page, "Email address")).toHaveJSProperty("readOnly", true);
});

test("offline says so, holds the check and picks it up again", async ({ page }) => {
  const { discoveries } = await openCard(page, { discover: [FASTMAIL_FOUND] });
  await page.context().setOffline(true);
  await expect(page.getByRole("status")).toContainText("You’re offline");
  await field(page, "Email address").fill(FASTMAIL_ADDRESS);
  await page.getByRole("button", { name: "Continue" }).click();
  await expect(card(page)).toHaveAttribute("data-step", "detecting");
  expect(discoveries).toEqual([]);
  await page.context().setOffline(false);
  await expect(card(page)).toHaveAttribute("data-step", "found");
  expect(discoveries).toEqual([FASTMAIL_ADDRESS]);
});

test("a reconnect asks for the credential only and replaces it on the row", async ({ page }) => {
  await mockSignedIn(page);
  const stopped = accountRow(FIXED_NOW, {
    stoppedCause: "credentials",
    stoppedAt: FIXED_NOW.getTime(),
  });
  const { credentials } = await mockAccounts(page, [stopped]);
  await page.goto("/accounts/new?reconnect=acc-1");
  await expect(page.getByRole("heading", { level: 1, name: "Reconnect Fastmail" })).toBeVisible();
  await expect(field(page, "Email address")).toHaveValue(FASTMAIL_ADDRESS);
  await expect(field(page, "Email address")).toHaveJSProperty("readOnly", true);
  await expect(page.getByRole("button", { name: "Change" })).toHaveCount(0);
  await expect(field(page, "API token")).toBeFocused();
  await field(page, "API token").fill(TOKEN);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByText("Fastmail connected.")).toBeVisible();
  // A reconnect the mail screen did not send returns to the accounts page.
  await expect(page).toHaveURL(/\/settings\/accounts$/);
  expect(credentials).toEqual([{ id: "acc-1", credential: { kind: "bearer", token: TOKEN } }]);
});

test("an unknown id at the reconnect address opens the card plain", async ({ page }) => {
  await mockSignedIn(page);
  await mockAccounts(page, [accountRow(FIXED_NOW)]);
  await page.goto("/accounts/new?reconnect=acc-9");
  await expect(page.getByRole("heading", { level: 1, name: "Add a mail account" })).toBeVisible();
});

test("a session that ended meanwhile signs out and says so", async ({ page }) => {
  await openCard(page, { discover: [{ status: 401, error: "unauthenticated" }] });
  await field(page, "Email address").fill(FASTMAIL_ADDRESS);
  await page.getByRole("button", { name: "Continue" }).click();
  await expect(page).toHaveURL(/\/sign-in$/);
  await expect(page.getByText("Your session has ended. Sign in again.")).toBeVisible();
});

test("the card is axe-clean and matches its screenshots", async ({ page }) => {
  for (const viewport of VIEWPORTS) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    for (const theme of THEMES) {
      await test.step(`in ${theme} at ${viewport.name} width`, async () => {
        await page.emulateMedia({ colorScheme: theme });
        await openCard(page);
        await page.evaluate(async () => {
          await document.fonts.ready;
        });
        const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
        expect.soft(results.violations, `axe in ${theme} at ${viewport.name} width`).toEqual([]);
        await expect.soft(page).toHaveScreenshot(`add-account-${theme}-${viewport.name}.png`, {
          fullPage: true,
        });
      });
    }
  }
});

test("the card reads in Dutch and in the pseudo-locale", async ({ page }) => {
  const desktop = VIEWPORTS[1];
  await page.setViewportSize({ width: desktop.width, height: desktop.height });
  await mockSignedIn(page);
  await mockAccounts(page, []);
  await mockPreferences(page, { locale: "nl" });
  await page.goto("/accounts/new");
  await expect(
    page.getByRole("heading", { level: 1, name: "Een mailaccount toevoegen" }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: "Doorgaan" })).toBeVisible();
  await expect.soft(page).toHaveScreenshot("add-account-nl-light-desktop.png", { fullPage: true });
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "en-XA");
  });
  await page.goto("/accounts/new");
  await expect(page.getByRole("heading", { level: 1, name: /Ádd á máíl/ })).toBeVisible();
  await expect.soft(page).toHaveScreenshot("add-account-en-XA-light-desktop.png", {
    fullPage: true,
  });
});
