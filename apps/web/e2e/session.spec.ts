// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { AxeBuilder } from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { mockSessionFlow, mockSignedIn, mockSignedOut } from "./session-mocks";

const ATTRIBUTION = "Huliho, by Eric Kochen";
const COPYRIGHT_LINE = "Copyright (C) 2026 Eric Kochen";
const THEMES = ["light", "dark"] as const;
const VIEWPORTS = [
  { name: "phone", width: 390, height: 844 },
  { name: "desktop", width: 1440, height: 900 },
] as const;
const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];
const RETRY_SECONDS = 90;

async function expectNotices(page: Page): Promise<void> {
  await expect(page.getByText(ATTRIBUTION)).toBeVisible();
  await expect(page.getByText(COPYRIGHT_LINE)).toBeVisible();
  await expect(page.getByText("This program comes with absolutely no warranty.")).toBeVisible();
  await expect(
    page.getByText("Licensees may convey it under the GNU AGPL, version 3."),
  ).toBeVisible();
  await expect(page.getByRole("link", { name: "License" })).toHaveAttribute("href", "/license");
  await expect(page.getByRole("link", { name: "Source code" })).toHaveAttribute(
    "href",
    "https://github.com/huliho/huliho",
  );
}

async function signIn(page: Page): Promise<void> {
  await page.getByLabel("Name").fill("mira@example.com");
  await page.getByLabel("Password").fill("example passphrase");
  await page.getByRole("button", { name: "Sign in" }).click();
}

test("an unauthenticated visit lands on sign-in with the notices", async ({ page }) => {
  await mockSignedOut(page);
  await page.goto("/");
  await expect(page).toHaveURL(/\/sign-in$/);
  await expect(page.getByLabel("Name")).toBeVisible();
  await expect(page.getByLabel("Password")).toBeVisible();
  await expect(page.getByText("Accounts are created by your admin.")).toBeVisible();
  await expectNotices(page);
});

test("the sign-in screen is axe-clean and matches its screenshots", async ({ page }) => {
  await mockSignedOut(page);
  for (const viewport of VIEWPORTS) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    for (const theme of THEMES) {
      await test.step(`in ${theme} at ${viewport.name} width`, async () => {
        await page.emulateMedia({ colorScheme: theme });
        await page.goto("/sign-in");
        await page.evaluate(async () => {
          await document.fonts.ready;
        });
        const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
        expect.soft(results.violations, `axe in ${theme} at ${viewport.name} width`).toEqual([]);
        await expect.soft(page).toHaveScreenshot(`sign-in-${theme}-${viewport.name}.png`, {
          fullPage: true,
        });
      });
    }
  }
});

test("wrong credentials mark the password field and explain", async ({ page }) => {
  await mockSignedOut(page);
  await page.route("**/api/session", (route) => {
    if (route.request().method() === "POST") {
      return route.fulfill({ status: 401, json: { error: "invalid_credentials" } });
    }
    return route.fulfill({ status: 401, json: { error: "unauthenticated" } });
  });
  await page.goto("/sign-in");
  await page.getByLabel("Name").fill("mira@example.com");
  await page.getByLabel("Password").fill("example passphrase");
  await page.getByLabel("Password").press("Enter");
  await expect(page.getByText("Couldn’t sign in. Check the name and password")).toBeVisible();
  await expect(page.getByLabel("Password")).toHaveAttribute("aria-invalid", "true");
  await expect(page.getByLabel("Password")).toBeFocused();
  await expect.soft(page).toHaveScreenshot("sign-in-error-light-desktop.png", { fullPage: true });
});

test("a rate limited answer holds the form with a countdown", async ({ page }) => {
  await page.route("**/api/session", (route) => {
    if (route.request().method() === "POST") {
      return route.fulfill({
        status: 429,
        headers: { "retry-after": String(RETRY_SECONDS) },
        json: { error: "rate_limited" },
      });
    }
    return route.fulfill({ status: 401, json: { error: "unauthenticated" } });
  });
  await page.goto("/sign-in");
  await signIn(page);
  await expect(page.getByText("Too many attempts.", { exact: false })).toBeVisible();
  await expect(page.getByRole("button")).toContainText(/Try again in 0?1:/);
  await expect(page.getByLabel("Name")).toHaveJSProperty("readOnly", true);
});

test("an unreachable API shows the designed error state", async ({ page }) => {
  await page.route("**/api/session", (route) => route.abort());
  await page.goto("/");
  await expect(page.getByRole("alert")).toContainText("Couldn’t reach the server");
  await expect(page.getByRole("button", { name: "Try again" })).toBeVisible();
  await page.goto("/sign-in");
  await expect(page.getByLabel("Name")).toBeVisible();
});

test("signing in reaches the shell and survives a reload", async ({ page }) => {
  await mockSessionFlow(page);
  await page.goto("/sign-in");
  await signIn(page);
  await expect(page.getByRole("heading", { level: 1, name: "Huliho" })).toBeVisible();
  await page.reload();
  await expect(page.getByRole("heading", { level: 1, name: "Huliho" })).toBeVisible();
});

test("signing out returns to the sign-in screen", async ({ page }) => {
  await mockSessionFlow(page);
  await page.goto("/sign-in");
  await signIn(page);
  await expect(page.getByRole("heading", { level: 1, name: "Huliho" })).toBeVisible();
  await page.getByRole("button", { name: "Sign out" }).click();
  await expect(page).toHaveURL(/\/sign-in$/);
  await expect(page.getByLabel("Name")).toBeVisible();
});

test("the about view shows the notices and is axe-clean", async ({ page }) => {
  await mockSignedIn(page);
  for (const viewport of VIEWPORTS) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    for (const theme of THEMES) {
      await test.step(`in ${theme} at ${viewport.name} width`, async () => {
        await page.emulateMedia({ colorScheme: theme });
        await page.goto("/settings/about");
        await page.evaluate(async () => {
          await document.fonts.ready;
        });
        await expectNotices(page);
        const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
        expect.soft(results.violations, `axe in ${theme} at ${viewport.name} width`).toEqual([]);
        await expect.soft(page).toHaveScreenshot(`about-${theme}-${viewport.name}.png`, {
          fullPage: true,
        });
      });
    }
  }
});

test("escape leaves the about view for the shell", async ({ page }) => {
  await mockSignedIn(page);
  await page.goto("/settings/about");
  await expect(page.getByText(ATTRIBUTION)).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("heading", { level: 1, name: "Huliho" })).toBeVisible();
});

test("the notices render in Dutch with the names verbatim", async ({ page }) => {
  await mockSignedOut(page);
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "nl");
  });
  await page.goto("/sign-in");
  await expect(page.getByText(ATTRIBUTION)).toBeVisible();
  await expect(page.getByText("Dit programma wordt geleverd zonder enige garantie.")).toBeVisible();
  await expect(page.getByRole("link", { name: "Licentie" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Inloggen" })).toBeVisible();
});
