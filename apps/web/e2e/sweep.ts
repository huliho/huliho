// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { AxeBuilder } from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { expect } from "@playwright/test";

// Every sweep captures both themes at the phone and desktop reference
// widths and holds the page to the same WCAG tags.
export const THEMES = ["light", "dark"] as const;
export const VIEWPORTS = [
  { name: "phone", width: 390, height: 844 },
  { name: "desktop", width: 1440, height: 900 },
] as const;
export const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];

// The frame a message renders in.
const MAIL_FRAME = "iframe[sandbox]";

// The axe pass over a page that may show a message. The frame holds
// sender content and runs no script, so the checker cannot enter it;
// the frame's own name is asserted where a test opens one.
export function axeOn(page: Page): AxeBuilder {
  return new AxeBuilder({ page }).withTags(WCAG_TAGS).exclude(MAIL_FRAME);
}

// Every message frame on the page has loaded its document and stands at
// its height, so a screenshot shows the message and not the still lines.
export async function framesLoaded(page: Page): Promise<void> {
  await page.waitForFunction(() => document.querySelector("iframe[data-loading]") === null);
}

// At the desktop width the sidebar's New folder control stands once the
// account's limits are in, which land after the rows on a repeat load;
// the narrower layouts keep the sidebar in a sheet. The title signal
// waits instead for the mailbox's name to be its menu's button, which
// its folder items make at every width.
export async function limitsIn(page: Page, signal: "sidebar" | "title" = "sidebar"): Promise<void> {
  if (signal === "title") {
    await expect(page.locator("main header h1").getByRole("button")).toBeVisible();
    return;
  }
  const narrower = page.locator('[data-layout]:not([data-layout="desktop"])');
  const newFolder = page
    .locator('[data-layout="desktop"]')
    .getByRole("tree")
    .locator("..")
    .getByRole("button");
  await expect(narrower.or(newFolder)).toBeVisible();
}
