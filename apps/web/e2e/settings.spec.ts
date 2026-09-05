// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { AxeBuilder } from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { mockSessions, mockSignedIn, sessionRows } from "./session-mocks";

// Screenshots must not age, so the page renders a pinned date.
const FIXED_NOW = new Date("2026-05-14T10:00:00");
// The undo window plus a margin, so an expired timer has certainly fired.
const UNDO_WINDOW_MS = 5_000;
const PAST_WINDOW_MS = UNDO_WINDOW_MS + 1_000;
const LIST_DELAY_MS = 1_500;
const THEMES = ["light", "dark"] as const;
const VIEWPORTS = [
  { name: "phone", width: 390, height: 844 },
  { name: "desktop", width: 1440, height: 900 },
] as const;
const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];

async function openSessions(page: Page): Promise<{ deletes: string[] }> {
  const mocks = await mockSessions(page, sessionRows(FIXED_NOW));
  await page.clock.install({ time: FIXED_NOW });
  await page.goto("/settings/sessions");
  await expect(page.getByText("Active sessions")).toBeVisible();
  return mocks;
}

test("the list names each device, its address and when it was last seen", async ({ page }) => {
  await openSessions(page);
  await expect(page.getByText("This device, Firefox on Linux")).toBeVisible();
  await expect(page.getByText("203.0.113.7").first()).toBeVisible();
  await expect(page.getByText("active now")).toBeVisible();
  await expect(page.getByText("Phone, installed app, Android")).toBeVisible();
  await expect(page.getByText("2 hours ago")).toBeVisible();
  await expect(page.getByText("Safari on macOS")).toBeVisible();
  await expect(page.getByText("3 weeks ago")).toBeVisible();
  await expect(page.getByText("Current", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: /^Revoke (?!all others)/ })).toHaveCount(2);
});

test("revoking a session waits behind the toast and undo cancels it", async ({ page }) => {
  const { deletes } = await openSessions(page);
  await page.getByRole("button", { name: "Revoke Safari on macOS" }).click();
  await expect(page.getByText("Safari on macOS")).toBeHidden();
  await expect(page.getByText("Safari session revoked.")).toBeVisible();
  await page.getByRole("button", { name: "Undo" }).click();
  await expect(page.getByText("Safari on macOS")).toBeVisible();
  await page.clock.runFor(PAST_WINDOW_MS);
  expect(deletes).toEqual([]);
});

test("the z key undoes the latest revoke", async ({ page }) => {
  const { deletes } = await openSessions(page);
  await page.getByRole("button", { name: "Revoke Phone, installed app, Android" }).click();
  await expect(page.getByText("Phone session revoked.")).toBeVisible();
  await page.keyboard.press("z");
  await expect(page.getByText("Phone, installed app, Android")).toBeVisible();
  await page.clock.runFor(PAST_WINDOW_MS);
  expect(deletes).toEqual([]);
});

test("a keyboard revoke hands focus to a neighboring row, then to the list", async ({ page }) => {
  await openSessions(page);
  const phone = page.getByRole("button", { name: "Revoke Phone, installed app, Android" });
  await phone.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("button", { name: "Revoke Safari on macOS" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("list")).toBeFocused();
  await expect(page.getByRole("button", { name: "Revoke all others" })).toBeHidden();
});

test("when the toast runs out the server hears about it", async ({ page }) => {
  const { deletes } = await openSessions(page);
  await page.getByRole("button", { name: "Revoke Safari on macOS" }).click();
  await page.clock.runFor(PAST_WINDOW_MS);
  await expect(page.getByText("Safari session revoked.")).toBeHidden();
  expect(deletes).toEqual(["s-mac"]);
});

test("hovering the toast pauses it and says so", async ({ page }) => {
  const { deletes } = await openSessions(page);
  await page.getByRole("button", { name: "Revoke Safari on macOS" }).click();
  const toast = page.getByRole("dialog");
  await toast.hover();
  await expect(toast.getByText("Paused")).toBeVisible();
  await page.clock.runFor(PAST_WINDOW_MS);
  await expect(toast).toBeVisible();
  expect(deletes).toEqual([]);
  await page.mouse.move(0, 0);
  await page.clock.runFor(PAST_WINDOW_MS);
  await expect(toast).toBeHidden();
  expect(deletes).toEqual(["s-mac"]);
});

test("revoking all others removes every other row and hits the collection", async ({ page }) => {
  const { deletes } = await openSessions(page);
  await page.getByRole("button", { name: "Revoke all others" }).click();
  await expect(page.getByText("2 other sessions revoked.")).toBeVisible();
  await expect(page.getByRole("button", { name: /^Revoke (?!all others)/ })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Revoke all others" })).toBeHidden();
  await expect(page.getByRole("list")).toBeFocused();
  await page.clock.runFor(PAST_WINDOW_MS);
  expect(deletes).toEqual(["*"]);
});

test("a slow list shows skeleton rows first", async ({ page }) => {
  await mockSignedIn(page);
  await page.route("**/api/sessions", async (route) => {
    await new Promise((resolve) => {
      setTimeout(resolve, LIST_DELAY_MS);
    });
    await route.fulfill({ json: sessionRows(FIXED_NOW) });
  });
  await page.goto("/settings/sessions");
  await expect(page.getByLabel("Loading…")).toBeVisible();
  await expect(page.getByText("This device, Firefox on Linux")).toBeVisible();
});

test("a failed list offers a retry that works", async ({ page }) => {
  await mockSessions(page, sessionRows(FIXED_NOW), [500]);
  await page.goto("/settings/sessions");
  await expect(page.getByRole("alert")).toContainText("Couldn’t load your sessions.");
  await page.getByRole("button", { name: "Try again" }).click();
  await expect(page.getByText("This device, Firefox on Linux")).toBeVisible();
});

test("escape leaves settings for the shell", async ({ page }) => {
  await openSessions(page);
  await page.keyboard.press("Escape");
  await expect(page.getByRole("heading", { level: 1, name: "Huliho" })).toBeVisible();
});

test("a phone lists the pages first and a wide screen opens the first page", async ({ page }) => {
  await mockSessions(page, sessionRows(FIXED_NOW));
  await page.setViewportSize(VIEWPORTS[0]);
  await page.goto("/settings");
  await expect(page.getByRole("heading", { level: 1, name: "Settings" })).toBeVisible();
  await page.getByRole("link", { name: "Sessions & devices" }).click();
  await expect(page).toHaveURL(/\/settings\/sessions$/);
  await expect(page.getByRole("heading", { level: 1, name: "Sessions & devices" })).toBeVisible();
  await page.getByRole("link", { name: "Back" }).click();
  await expect(page).toHaveURL(/\/settings$/);
  await page.setViewportSize(VIEWPORTS[1]);
  await page.goto("/settings");
  await expect(page).toHaveURL(/\/settings\/sessions$/);
  await expect(page.getByRole("heading", { level: 1, name: "Settings" })).toBeVisible();
});

test("the page is axe-clean and matches its screenshots", async ({ page }) => {
  for (const viewport of VIEWPORTS) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    for (const theme of THEMES) {
      await test.step(`in ${theme} at ${viewport.name} width`, async () => {
        await page.emulateMedia({ colorScheme: theme });
        await openSessions(page);
        await page.evaluate(async () => {
          await document.fonts.ready;
        });
        const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
        expect.soft(results.violations, `axe in ${theme} at ${viewport.name} width`).toEqual([]);
        await expect.soft(page).toHaveScreenshot(`sessions-${theme}-${viewport.name}.png`, {
          fullPage: true,
        });
      });
    }
  }
});

test("the translated pages match their screenshots and the region is named", async ({ page }) => {
  const desktop = VIEWPORTS[1];
  await page.setViewportSize({ width: desktop.width, height: desktop.height });
  await mockSessions(page, sessionRows(FIXED_NOW));
  await page.clock.install({ time: FIXED_NOW });
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "nl");
  });
  await page.goto("/settings/sessions");
  await expect(page.getByText("Dit apparaat, Firefox op Linux")).toBeVisible();
  await expect(page.getByText("2 uur geleden")).toBeVisible();
  await expect(page.getByRole("region", { name: "Meldingen" })).toBeAttached();
  await expect.soft(page).toHaveScreenshot("sessions-nl-light-desktop.png", { fullPage: true });
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "en-XA");
  });
  await page.goto("/settings/sessions");
  await expect(page.getByText(/Áçtívé séssíóñs/)).toBeVisible();
  await expect.soft(page).toHaveScreenshot("sessions-en-XA-light-desktop.png", { fullPage: true });
});
