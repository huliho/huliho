// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { AxeBuilder } from "@axe-core/playwright";
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { mockSessions, mockSignedIn, sessionRows } from "./session-mocks";
import type { MockRole } from "./session-mocks";
import { THEMES, VIEWPORTS, WCAG_TAGS } from "./sweep";
import { ISSUED_ONE_TIME, mockUsers, userRows } from "./user-mocks";
import type { CreateBody, UsersAnswers } from "./user-mocks";

// Screenshots must not age, so the page renders a pinned date.
const FIXED_NOW = new Date("2026-05-14T10:00:00");
const LIST_DELAY_MS = 1_500;

async function openUsers(
  page: Page,
  role: MockRole = "owner",
  answers: UsersAnswers = {},
): Promise<{ creates: CreateBody[]; resets: string[] }> {
  await mockSignedIn(page, role);
  const mocks = await mockUsers(page, userRows(FIXED_NOW), answers);
  await page.clock.install({ time: FIXED_NOW });
  await page.goto("/settings/users");
  await expect(page.getByText("jonas@example.com")).toBeVisible();
  return mocks;
}

function dialog(page: Page): Locator {
  return page.getByRole("dialog");
}

test("the table names each user, their role and when they were last active", async ({ page }) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await openUsers(page);
  await expect(page.getByRole("columnheader", { name: "Name", exact: true })).toBeVisible();
  await expect(page.getByRole("columnheader", { name: "Last active" })).toBeVisible();
  await expect(page.getByRole("row", { name: /Jonas/ })).toContainText("yesterday");
  await expect(page.getByRole("row", { name: /Mira/ })).toContainText("active now");
  await expect(page.getByRole("row", { name: /Mira/ })).toContainText("You");
  await expect(page.getByRole("row", { name: /Noor/ })).toContainText("Never");
  await expect(page.getByRole("row", { name: /Tomas/ })).toContainText("3 weeks ago");
  await expect(page.getByText("Owner", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: /^Reset password for/ })).toHaveCount(3);
  await expect(page.getByRole("button", { name: "Reset password for Mira" })).toHaveCount(0);
});

test("a phone lists one card per user with the same facts", async ({ page }) => {
  await page.setViewportSize(VIEWPORTS[0]);
  await openUsers(page);
  await expect(page.getByRole("table")).toHaveCount(0);
  await expect(page.getByRole("list", { name: "Users" }).getByRole("listitem")).toHaveCount(4);
  await expect(page.getByText("3 weeks ago")).toBeVisible();
  await expect(page.getByRole("button", { name: /^Reset password for/ })).toHaveCount(3);
});

test("a reset asks first, shows the password once and hands focus back", async ({
  page,
  context,
}) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  const { resets } = await openUsers(page);
  const trigger = page.getByRole("button", { name: "Reset password for Jonas" });
  await trigger.click();
  await expect(
    dialog(page).getByRole("heading", { name: "Reset Jonas’s password?" }),
  ).toBeVisible();
  await expect(dialog(page).getByRole("button", { name: "Cancel" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog(page)).toBeHidden();
  expect(resets).toEqual([]);
  await trigger.click();
  await dialog(page).getByRole("button", { name: "Reset password" }).click();
  await expect(dialog(page).getByRole("heading", { name: "Give this to Jonas" })).toBeVisible();
  await expect(dialog(page).getByText(ISSUED_ONE_TIME)).toBeVisible();
  await expect(dialog(page)).toContainText("It works for one sign-in until");
  await expect(dialog(page)).toContainText("Every session of Jonas is now signed out.");
  await expect(dialog(page).getByRole("button", { name: "Copy" })).toBeFocused();
  await dialog(page).getByRole("button", { name: "Copy" }).click();
  await expect(dialog(page).getByText("Copied")).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(ISSUED_ONE_TIME);
  await dialog(page).getByRole("button", { name: "Done" }).click();
  await expect(dialog(page)).toBeHidden();
  await expect(page.getByText(ISSUED_ONE_TIME)).toHaveCount(0);
  await expect(trigger).toBeFocused();
  expect(resets).toEqual(["user-2"]);
});

test("a refused reset says so and can be tried again", async ({ page }) => {
  const { resets } = await openUsers(page, "owner", { reset: [{ status: 500 }] });
  await page.getByRole("button", { name: "Reset password for Jonas" }).click();
  await dialog(page).getByRole("button", { name: "Reset password" }).click();
  await expect(dialog(page).getByRole("alert")).toContainText("Couldn’t reset the password.");
  await dialog(page).getByRole("button", { name: "Reset password" }).click();
  await expect(dialog(page).getByText(ISSUED_ONE_TIME)).toBeVisible();
  expect(resets).toEqual(["user-2", "user-2"]);
});

test("creating a user hands over the first password and lists the user", async ({ page }) => {
  const { creates } = await openUsers(page);
  const trigger = page.getByRole("button", { name: "Create user" });
  await trigger.click();
  await expect(dialog(page).getByRole("heading", { name: "Create a user" })).toBeVisible();
  await expect(dialog(page).getByLabel("Name", { exact: true })).toBeFocused();
  await dialog(page).getByLabel("Name", { exact: true }).fill("Sara");
  await dialog(page).getByLabel("Sign-in name").fill("sara@example.com");
  await dialog(page).getByLabel("Role").selectOption("admin");
  await dialog(page).getByRole("button", { name: "Create" }).click();
  await expect(dialog(page).getByRole("heading", { name: "Give this to Sara" })).toBeVisible();
  await expect(dialog(page)).not.toContainText("signed out");
  await dialog(page).getByRole("button", { name: "Done" }).click();
  await expect(page.getByText("sara@example.com")).toBeVisible();
  await expect(trigger).toBeFocused();
  expect(creates).toEqual([{ name: "Sara", login: "sara@example.com", role: "admin" }]);
});

test("a sign-in name with a space never leaves the browser and a taken one is named", async ({
  page,
}) => {
  const { creates } = await openUsers(page, "owner", {
    create: [{ status: 409, error: "login_taken" }],
  });
  await page.getByRole("button", { name: "Create user" }).click();
  await dialog(page).getByLabel("Name", { exact: true }).fill("Sara");
  await dialog(page).getByLabel("Sign-in name").fill("sara example");
  await dialog(page).getByRole("button", { name: "Create" }).click();
  await expect(dialog(page).getByRole("alert")).toContainText("without spaces");
  await expect(dialog(page).getByLabel("Sign-in name")).toBeFocused();
  expect(creates).toEqual([]);
  await dialog(page).getByLabel("Sign-in name").fill("jonas@example.com");
  await dialog(page).getByRole("button", { name: "Create" }).click();
  await expect(dialog(page).getByRole("alert")).toContainText("already in use");
  await expect(dialog(page).getByLabel("Sign-in name")).toHaveAttribute("aria-invalid", "true");
  await dialog(page).getByLabel("Sign-in name").fill("sara@example.com");
  await expect(dialog(page).getByRole("alert")).toHaveCount(0);
  expect(creates).toHaveLength(1);
});

test("an admin resets no owner and grants member or admin; an owner grants owner too", async ({
  page,
}) => {
  await openUsers(page, "admin");
  await expect(page.getByRole("row", { name: /Tomas/ })).toContainText("You");
  await expect(page.getByRole("button", { name: "Reset password for Mira" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: /^Reset password for/ })).toHaveCount(2);
  await page.getByRole("button", { name: "Create user" }).click();
  await expect(dialog(page).getByRole("option")).toHaveText(["Member", "Admin"]);
  await page.keyboard.press("Escape");
  await mockSignedIn(page, "owner");
  await page.reload();
  await page.getByRole("button", { name: "Create user" }).click();
  await expect(dialog(page).getByRole("option")).toHaveText(["Member", "Admin", "Owner"]);
});

test("the Admin group shows for an owner and a member never reaches the page", async ({ page }) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await openUsers(page);
  const nav = page.getByRole("navigation", { name: "Settings" });
  await expect(nav.getByText("Admin", { exact: true })).toBeVisible();
  await expect(nav.getByRole("link", { name: "Users" })).toHaveAttribute("aria-current", "page");
  await mockSessions(page, sessionRows(FIXED_NOW));
  await mockSignedIn(page, "member");
  await page.goto("/settings/users");
  await expect(page).toHaveURL(/\/settings\/sessions$/);
  await expect(page.getByRole("heading", { level: 2, name: "Active sessions" })).toBeVisible();
  await expect(nav.getByRole("link", { name: "Users" })).toHaveCount(0);
  await expect(nav.getByText("Admin", { exact: true })).toHaveCount(0);
});

test("a slow list shows skeleton rows first", async ({ page }) => {
  await mockSignedIn(page);
  await page.route("**/api/users", async (route) => {
    await new Promise((resolve) => {
      setTimeout(resolve, LIST_DELAY_MS);
    });
    await route.fulfill({ json: userRows(FIXED_NOW) });
  });
  await page.goto("/settings/users");
  await expect(page.getByLabel("Loading…")).toBeVisible();
  await expect(page.getByText("jonas@example.com")).toBeVisible();
});

test("a failed list offers a retry that works", async ({ page }) => {
  await mockSignedIn(page);
  await mockUsers(page, userRows(FIXED_NOW), { list: [500] });
  await page.goto("/settings/users");
  await expect(page.getByRole("alert")).toContainText("Couldn’t load the users.");
  await page.getByRole("button", { name: "Try again" }).click();
  await expect(page.getByText("jonas@example.com")).toBeVisible();
});

test("a session that ended meanwhile signs the admin out and says so", async ({ page }) => {
  await openUsers(page, "owner", { reset: [{ status: 401, error: "unauthenticated" }] });
  await page.getByRole("button", { name: "Reset password for Jonas" }).click();
  await dialog(page).getByRole("button", { name: "Reset password" }).click();
  await expect(page).toHaveURL(/\/sign-in$/);
  await expect(page.getByText("Your session has ended. Sign in again.")).toBeVisible();
});

test("the page is axe-clean and matches its screenshots", async ({ page }) => {
  for (const viewport of VIEWPORTS) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    for (const theme of THEMES) {
      await test.step(`in ${theme} at ${viewport.name} width`, async () => {
        await page.emulateMedia({ colorScheme: theme });
        await openUsers(page);
        await page.evaluate(async () => {
          await document.fonts.ready;
        });
        const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
        expect.soft(results.violations, `axe in ${theme} at ${viewport.name} width`).toEqual([]);
        await expect.soft(page).toHaveScreenshot(`users-${theme}-${viewport.name}.png`, {
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
  await mockUsers(page, userRows(FIXED_NOW));
  await page.clock.install({ time: FIXED_NOW });
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "nl");
  });
  await page.goto("/settings/users");
  await expect(page.getByRole("columnheader", { name: "Inlognaam" })).toBeVisible();
  await expect(page.getByText("gisteren")).toBeVisible();
  await expect(page.getByRole("button", { name: "Gebruiker aanmaken" })).toBeVisible();
  await expect.soft(page).toHaveScreenshot("users-nl-light-desktop.png", { fullPage: true });
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "en-XA");
  });
  await page.goto("/settings/users");
  await expect(page.getByRole("heading", { level: 2, name: /Úsérs/ })).toBeVisible();
  await expect.soft(page).toHaveScreenshot("users-en-XA-light-desktop.png", { fullPage: true });
});
