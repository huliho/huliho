// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { AxeBuilder } from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import {
  mockForcedSession,
  mockPasswordChange,
  mockSessions,
  mockSignedOut,
  sessionRows,
} from "./session-mocks";
import type { PasswordAnswer, PasswordChangeBody } from "./session-mocks";
import { THEMES, VIEWPORTS, WCAG_TAGS } from "./sweep";

// Screenshots must not age, so the page renders a pinned date.
const FIXED_NOW = new Date("2026-05-14T10:00:00");
const CURRENT = "the old passphrase";
const NEXT = "a brand new passphrase";
const RETRY_SECONDS = 90;
const CHANGED_TOAST = "Password changed. Other devices were signed out.";

async function openPasswordSection(
  page: Page,
  answers: PasswordAnswer[] = [],
): Promise<{ changes: PasswordChangeBody[] }> {
  await mockSessions(page, sessionRows(FIXED_NOW));
  const mocks = await mockPasswordChange(page, answers);
  await page.clock.install({ time: FIXED_NOW });
  await page.goto("/settings/sessions");
  await expect(page.getByRole("heading", { level: 2, name: "Password" })).toBeVisible();
  return mocks;
}

async function fillNewPassword(page: Page, next = NEXT, repeat = next): Promise<void> {
  await page.getByLabel("New password").fill(next);
  await page.getByLabel("Repeat it").fill(repeat);
}

async function fillAll(page: Page): Promise<void> {
  await page.getByLabel("Current password").fill(CURRENT);
  await fillNewPassword(page);
}

test("changing the password clears the form, says so and refreshes the list", async ({ page }) => {
  const { changes } = await openPasswordSection(page);
  // After the change the server lists only the current session; a fresh
  // list on the page proves the refetch.
  await page.route("**/api/sessions", (route) =>
    route.fulfill({ json: sessionRows(FIXED_NOW).filter((row) => row.current) }),
  );
  await fillAll(page);
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText(CHANGED_TOAST)).toBeVisible();
  expect(changes).toEqual([{ current: CURRENT, new: NEXT }]);
  await expect(page.getByLabel("Current password")).toHaveValue("");
  await expect(page.getByLabel("New password")).toHaveValue("");
  await expect(page.getByLabel("Repeat it")).toHaveValue("");
  await expect(page.getByRole("button", { name: "Save" })).toBeFocused();
  await expect(page.getByText("Safari on macOS")).toBeHidden();
});

test("a wrong current password marks that field and the limiter holds the form", async ({
  page,
}) => {
  await openPasswordSection(page, [
    { status: 401, error: "invalid_credentials" },
    { status: 429, error: "rate_limited", retryAfter: RETRY_SECONDS },
  ]);
  await fillAll(page);
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("alert")).toContainText("Check your current password");
  await expect(page.getByLabel("Current password")).toHaveAttribute("aria-invalid", "true");
  await page.getByLabel("Current password").fill(CURRENT);
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Too many attempts.", { exact: false })).toBeVisible();
  await expect(page.getByRole("button", { name: /^Try again in / })).toBeVisible();
  await expect(page.getByLabel("New password")).toHaveJSProperty("readOnly", true);
});

test("a session that ended meanwhile signs the user out and says so", async ({ page }) => {
  await openPasswordSection(page, [{ status: 401, error: "unauthenticated" }]);
  await fillAll(page);
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page).toHaveURL(/\/sign-in$/);
  await expect(page.getByText("Your session has ended. Sign in again.")).toBeVisible();
  await expect(page.getByLabel("Name")).toBeVisible();
});

test("a short or mismatched password never leaves the browser", async ({ page }) => {
  const { changes } = await openPasswordSection(page);
  await page.getByLabel("Current password").fill(CURRENT);
  await fillNewPassword(page, "too short");
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("alert")).toContainText("Use 12 to 128 characters.");
  await expect(page.getByLabel("New password")).toBeFocused();
  await fillNewPassword(page, NEXT, `${NEXT}!`);
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("alert")).toContainText("don’t match");
  await expect(page.getByLabel("Repeat it")).toBeFocused();
  expect(changes).toEqual([]);
});

test("a one-time password lands on the forced step from every route", async ({ page }) => {
  const { changes } = await mockForcedSession(page);
  for (const path of ["/settings/sessions", "/", "/sign-in"]) {
    await page.goto(path);
    await expect(page).toHaveURL(/\/choose-password$/);
  }
  await expect(
    page.getByRole("heading", { level: 2, name: "Choose a new password" }),
  ).toBeVisible();
  await expect(page.getByText("An admin reset your password", { exact: false })).toBeVisible();
  await expect(page.getByLabel("Current password")).toHaveCount(0);
  await fillNewPassword(page);
  await page.getByRole("button", { name: "Save and continue" }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Huliho" })).toBeVisible();
  await expect(page).not.toHaveURL(/choose-password/);
  await expect(page.getByText(CHANGED_TOAST)).toBeVisible();
  expect(changes).toEqual([{ new: NEXT }]);
});

test("the forced step can still sign out", async ({ page }) => {
  await mockForcedSession(page);
  await page.goto("/");
  await expect(page).toHaveURL(/\/choose-password$/);
  await page.getByRole("button", { name: "Sign out" }).click();
  await expect(page).toHaveURL(/\/sign-in$/);
  await expect(page.getByLabel("Name")).toBeVisible();
});

test("an ordinary session and a missing one never see the forced step", async ({ page }) => {
  await mockSessions(page, sessionRows(FIXED_NOW));
  await page.goto("/choose-password");
  await expect(page.getByRole("heading", { level: 1, name: "Huliho" })).toBeVisible();
  await expect(page).not.toHaveURL(/choose-password/);
  await mockSignedOut(page);
  await page.goto("/choose-password");
  await expect(page).toHaveURL(/\/sign-in$/);
});

test("the forced step is axe-clean and matches its screenshots", async ({ page }) => {
  await mockForcedSession(page);
  for (const viewport of VIEWPORTS) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    for (const theme of THEMES) {
      await test.step(`in ${theme} at ${viewport.name} width`, async () => {
        await page.emulateMedia({ colorScheme: theme });
        await page.goto("/choose-password");
        await expect(page.getByLabel("New password")).toBeVisible();
        await page.evaluate(async () => {
          await document.fonts.ready;
        });
        const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
        expect.soft(results.violations, `axe in ${theme} at ${viewport.name} width`).toEqual([]);
        await expect.soft(page).toHaveScreenshot(`choose-password-${theme}-${viewport.name}.png`, {
          fullPage: true,
        });
      });
    }
  }
});

test("the forced step reads in Dutch and in the pseudo-locale", async ({ page }) => {
  const desktop = VIEWPORTS[1];
  await page.setViewportSize({ width: desktop.width, height: desktop.height });
  await mockForcedSession(page);
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "nl");
  });
  await page.goto("/choose-password");
  await expect(
    page.getByRole("heading", { level: 2, name: "Kies een nieuw wachtwoord" }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: "Opslaan en doorgaan" })).toBeVisible();
  await expect.soft(page).toHaveScreenshot("choose-password-nl-light-desktop.png", {
    fullPage: true,
  });
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "en-XA");
  });
  await page.goto("/choose-password");
  await expect(page.getByRole("heading", { level: 2, name: /Çhóósé/ })).toBeVisible();
  await expect.soft(page).toHaveScreenshot("choose-password-en-XA-light-desktop.png", {
    fullPage: true,
  });
});
