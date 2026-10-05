// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { AxeBuilder } from "@axe-core/playwright";
import type { Page } from "@playwright/test";

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
