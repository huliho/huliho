// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { AxeBuilder } from "@axe-core/playwright";
import type { APIRequestContext, Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { STORYBOOK_URL } from "../playwright.config";

// The sweep must fail loudly when a rename leaves it iterating nothing.
const MIN_STORY_COUNT = 9;
const PER_STORY_TIMEOUT_MS = 15000;
const THEMES = ["light", "dark"] as const;
// The phone and desktop reference widths every sheet is captured at.
const VIEWPORTS = [
  { name: "phone", width: 390, height: 844 },
  { name: "desktop", width: 1440, height: 900 },
] as const;
const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function extractStoryIds(payload: unknown): string[] {
  if (!isRecord(payload) || !isRecord(payload.entries)) {
    return [];
  }
  const ids: string[] = [];
  for (const entry of Object.values(payload.entries)) {
    if (isRecord(entry) && entry.type === "story" && typeof entry.id === "string") {
      ids.push(entry.id);
    }
  }
  return ids;
}

async function storyIds(request: APIRequestContext): Promise<string[]> {
  const response = await request.get(`${STORYBOOK_URL}/index.json`);
  expect(response.ok()).toBe(true);
  const payload: unknown = await response.json();
  return extractStoryIds(payload);
}

async function auditStory(page: Page, id: string, theme: string, name: string): Promise<void> {
  await page.goto(`${STORYBOOK_URL}/iframe.html?id=${id}&globals=theme:${theme}`);
  // A story that fails to load never sets the theme; the wait must end with the step.
  await page.waitForFunction(
    (expected) => document.documentElement.dataset["theme"] === expected,
    theme,
    { timeout: PER_STORY_TIMEOUT_MS },
  );
  await page.evaluate(async () => {
    await document.fonts.ready;
  });
  // Overlays fade in; axe and the screenshot wait for the fade to end.
  await page.evaluate(async () => {
    await Promise.all(document.getAnimations().map((animation) => animation.finished));
  });
  const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
  expect.soft(results.violations, `axe on ${id} in ${theme} at ${name} width`).toEqual([]);
  await expect.soft(page).toHaveScreenshot(`${id}-${theme}-${name}.png`, { fullPage: true });
}

test("every token demo story is axe-clean and matches its screenshot", async ({
  page,
  request,
}) => {
  const ids = await storyIds(request);
  expect(ids.length).toBeGreaterThanOrEqual(MIN_STORY_COUNT);
  test.setTimeout(ids.length * THEMES.length * VIEWPORTS.length * PER_STORY_TIMEOUT_MS);
  for (const viewport of VIEWPORTS) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    for (const theme of THEMES) {
      for (const id of ids) {
        await test.step(`${id} in ${theme} at ${viewport.name} width`, async () => {
          await auditStory(page, id, theme, viewport.name);
        });
      }
    }
  }
});
