// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { APIRequestContext, Browser, Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { STORYBOOK_URL } from "../playwright.config";
import { THEMES, VIEWPORTS, axeOn, framesLoaded } from "./sweep";

// The sweep must fail loudly when a rename leaves it iterating nothing.
const MIN_STORY_COUNT = 9;
const PER_STORY_TIMEOUT_MS = 15000;

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
  await framesLoaded(page);
  const results = await axeOn(page).analyze();
  expect.soft(results.violations, `axe on ${id} in ${theme} at ${name} width`).toEqual([]);
  await expect.soft(page).toHaveScreenshot(`${id}-${theme}-${name}.png`, { fullPage: true });
}

// A tab's renderer grows with every story it draws, so each pass takes a
// context of its own and starts on a fresh renderer.
async function auditPass(
  browser: Browser,
  ids: readonly string[],
  viewport: (typeof VIEWPORTS)[number],
  theme: string,
): Promise<void> {
  const context = await browser.newContext({
    viewport: { width: viewport.width, height: viewport.height },
  });
  try {
    const page = await context.newPage();
    for (const id of ids) {
      await test.step(`${id} in ${theme} at ${viewport.name} width`, async () => {
        await auditStory(page, id, theme, viewport.name);
      });
    }
  } finally {
    await context.close();
  }
}

test("every token demo story is axe-clean and matches its screenshot", async ({
  browser,
  request,
}) => {
  const ids = await storyIds(request);
  expect(ids.length).toBeGreaterThanOrEqual(MIN_STORY_COUNT);
  test.setTimeout(ids.length * THEMES.length * VIEWPORTS.length * PER_STORY_TIMEOUT_MS);
  for (const viewport of VIEWPORTS) {
    for (const theme of THEMES) {
      await auditPass(browser, ids, viewport, theme);
    }
  }
});
