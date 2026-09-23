// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { AxeBuilder } from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { mockPreferences } from "./preference-mocks";
import { mockSignedIn } from "./session-mocks";
import { THEMES, VIEWPORTS, WCAG_TAGS } from "./sweep";

const PAGE = "/settings/appearance";
// Screenshots must not age, so the shell renders a pinned date.
const FIXED_NOW = new Date("2026-05-14T10:00:00");

async function openAppearance(page: Page, locale?: string): Promise<void> {
  if (locale !== undefined) {
    await page.addInitScript((value) => {
      window.localStorage.setItem("PARAGLIDE_LOCALE", value);
    }, locale);
  }
  await page.goto(PAGE);
  await page.evaluate(async () => {
    await document.fonts.ready;
  });
}

function checked(page: Page, group: string): ReturnType<Page["getByRole"]> {
  return page.getByRole("radiogroup", { name: group }).getByRole("radio", { checked: true });
}

test("the locale follows the browser preference until the user chooses", async ({ browser }) => {
  const context = await browser.newContext({ locale: "nl-NL" });
  const page = await context.newPage();
  await mockSignedIn(page);
  await openAppearance(page);
  await expect(page.getByRole("heading", { level: 2, name: "Thema" })).toBeVisible();
  await expect(checked(page, "Taal")).toHaveText("Nederlands");
  await expect(page.locator("html")).toHaveAttribute("lang", "nl");
  await context.close();
});

test("choosing a language translates every screen, sticks and reaches the server", async ({
  page,
}) => {
  await mockSignedIn(page);
  const { writes } = await mockPreferences(page);
  await page.clock.setFixedTime(FIXED_NOW);
  await openAppearance(page);
  await expect(page.getByRole("heading", { level: 2, name: "Theme" })).toBeVisible();

  await page.getByRole("radio", { name: "Nederlands" }).click();
  await expect(page.getByRole("heading", { level: 2, name: "Thema" })).toBeVisible();
  await expect(page.getByRole("heading", { level: 1, name: "Instellingen" })).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("lang", "nl");
  expect(writes).toEqual([{ key: "locale", value: "nl" }]);

  await page.goto("/");
  await expect(page.getByText("Je mail, waar die ook staat.")).toBeVisible();
  await expect(page.getByText(/24\.817 berichten/)).toBeVisible();
  await expect(page.getByText(/Vandaag is het donderdag 14 mei 2026/)).toBeVisible();

  await page.reload();
  await expect(page.getByText("Je mail, waar die ook staat.")).toBeVisible();
});

test("the choices on record follow the user to a device that has none", async ({ page }) => {
  await mockSignedIn(page);
  await mockPreferences(page, { theme: "dark", density: "compact", locale: "nl" });
  await openAppearance(page);
  await expect(page.getByRole("heading", { level: 2, name: "Thema" })).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await expect(page.locator("html")).toHaveAttribute("data-density", "compact");
  await expect(checked(page, "Thema")).toHaveText("Donker");
  await expect(checked(page, "Dichtheid")).toHaveText("Compact");
  await expect(checked(page, "Leesvenster")).toHaveText("Rechts");
});

test("a choice by click or by arrow key applies at once and reaches the server", async ({
  page,
}) => {
  await mockSignedIn(page);
  const { writes } = await mockPreferences(page);
  await openAppearance(page);
  await expect(page.locator("html")).toHaveAttribute("data-theme", "system");
  await page.getByRole("radio", { name: "Dark" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await page.keyboard.press("ArrowLeft");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await expect(page.getByRole("radio", { name: "Light" })).toBeFocused();
  await page.getByRole("radio", { name: "Compact" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-density", "compact");
  await page.getByRole("radio", { name: "Off" }).click();
  await expect(checked(page, "Reading pane")).toHaveText("Off");
  await expect
    .poll(() => writes)
    .toEqual([
      { key: "theme", value: "dark" },
      { key: "theme", value: "light" },
      { key: "density", value: "compact" },
      { key: "readingPane", value: "off" },
    ]);
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await expect(checked(page, "Theme")).toHaveText("Light");
});

test("a refused save puts the word on record back and says so", async ({ page }) => {
  await mockSignedIn(page);
  await mockPreferences(page, {}, 500);
  await openAppearance(page);
  await page.getByRole("radio", { name: "Dark" }).click();
  await expect(page.getByText("Couldn’t save that setting. Try again.")).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "system");
  await expect(checked(page, "Theme")).toHaveText("System");
});

test("the pseudo locale renders catalog text accented and stays past the server's word", async ({
  page,
}) => {
  await mockSignedIn(page);
  await mockPreferences(page, { locale: "en" });
  await openAppearance(page, "en-XA");
  await expect(page.getByRole("heading", { level: 2, name: /Thémé/ })).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("lang", "en-XA");
});

test("the appearance page is axe-clean and matches its screenshots", async ({ page }) => {
  await mockSignedIn(page);
  for (const viewport of VIEWPORTS) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    for (const theme of THEMES) {
      await test.step(`in ${theme} at ${viewport.name} width`, async () => {
        await page.emulateMedia({ colorScheme: theme });
        await openAppearance(page);
        await expect(page.getByRole("heading", { level: 2, name: "Theme" })).toBeVisible();
        const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
        expect.soft(results.violations, `axe in ${theme} at ${viewport.name} width`).toEqual([]);
        await expect.soft(page).toHaveScreenshot(`appearance-${theme}-${viewport.name}.png`, {
          fullPage: true,
        });
      });
    }
  }
});

test("the translated pages match their screenshots", async ({ page }) => {
  const desktop = VIEWPORTS[1];
  await page.setViewportSize({ width: desktop.width, height: desktop.height });
  await mockSignedIn(page);
  await mockPreferences(page, { locale: "nl" });
  await openAppearance(page);
  await expect(page.getByRole("heading", { level: 2, name: "Thema" })).toBeVisible();
  await expect.soft(page).toHaveScreenshot("appearance-nl-light-desktop.png", { fullPage: true });
  await openAppearance(page, "en-XA");
  await expect(page.getByRole("heading", { level: 2, name: /Thémé/ })).toBeVisible();
  await expect.soft(page).toHaveScreenshot("appearance-en-XA-light-desktop.png", {
    fullPage: true,
  });
});
