// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { AxeBuilder } from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

const EN_TAGLINE = "Your mail, wherever it lives.";
const NL_TAGLINE = "Je mail, waar die ook staat.";
// Screenshots must not age, so the page renders a pinned date.
const FIXED_NOW = new Date("2026-05-14T10:00:00");
const THEMES = ["light", "dark"] as const;
// The phone and desktop reference widths every page is captured at.
const VIEWPORTS = [
  { name: "phone", width: 390, height: 844 },
  { name: "desktop", width: 1440, height: 900 },
] as const;
const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];

async function openPinned(page: Page, locale?: string): Promise<void> {
  if (locale !== undefined) {
    await page.addInitScript((value) => {
      window.localStorage.setItem("PARAGLIDE_LOCALE", value);
    }, locale);
  }
  await page.clock.setFixedTime(FIXED_NOW);
  await page.goto("/");
  await page.evaluate(async () => {
    await document.fonts.ready;
  });
}

test("the locale follows the browser preference until the user chooses", async ({ browser }) => {
  const context = await browser.newContext({ locale: "nl-NL" });
  const page = await context.newPage();
  await openPinned(page);
  await expect(page.getByText(NL_TAGLINE)).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("lang", "nl");
  await context.close();
});

test("switching the locale translates the page and survives a reload", async ({ page }) => {
  await openPinned(page);
  await expect(page.getByText(EN_TAGLINE)).toBeVisible();
  await expect(page.getByText(/24,817 messages/)).toBeVisible();

  await page.getByLabel("Language").selectOption("nl");
  await expect(page.getByText(NL_TAGLINE)).toBeVisible();
  await expect(page.getByText(/24\.817 berichten/)).toBeVisible();
  await expect(page.getByText(/Vandaag is het donderdag 14 mei 2026/)).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("lang", "nl");

  await page.reload();
  await expect(page.getByText(NL_TAGLINE)).toBeVisible();
});

test("the pseudo locale renders catalog text accented", async ({ page }) => {
  await openPinned(page, "en-XA");
  await expect(page.getByText(/Ýóúr máíl/)).toBeVisible();
  await expect(page.getByRole("heading", { level: 1, name: "Huliho" })).toBeVisible();
});

test("the demo page is axe-clean and matches its screenshots", async ({ page }) => {
  for (const viewport of VIEWPORTS) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    for (const theme of THEMES) {
      await test.step(`in ${theme} at ${viewport.name} width`, async () => {
        await page.emulateMedia({ colorScheme: theme });
        await openPinned(page);
        const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
        expect.soft(results.violations, `axe in ${theme} at ${viewport.name} width`).toEqual([]);
        await expect.soft(page).toHaveScreenshot(`app-en-${theme}-${viewport.name}.png`, {
          fullPage: true,
        });
      });
    }
  }
});

test("the translated pages match their screenshots", async ({ page }) => {
  const desktop = VIEWPORTS[1];
  await page.setViewportSize({ width: desktop.width, height: desktop.height });
  for (const locale of ["nl", "en-XA"]) {
    await test.step(`in ${locale}`, async () => {
      await openPinned(page, locale);
      await expect.soft(page).toHaveScreenshot(`app-${locale}-light-desktop.png`, {
        fullPage: true,
      });
    });
  }
});
