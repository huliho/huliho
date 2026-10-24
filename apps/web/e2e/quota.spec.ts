// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import type { MockedMail } from "./mail-mocks";
import { THEMES, VIEWPORTS, axeOn } from "./sweep";
import { INBOX_PATH, openInbox, settled } from "./thread-pane";

const GB = 1000 ** 3;
const LIMIT = 10 * GB;
const TABLET = { width: 834, height: 1112 };
const PLAIN = "Mail storage: 4.2 GB of 10 GB used";
const NEARLY_FULL = "Mail storage: 9.1 GB of 10 GB used";

// The account's storage quota (RFC 9425 section 4.1), its figures whole
// octets as a server counts them.
function storage(used: number, extra: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: "q-storage",
    resourceType: "octets",
    used: Math.round(used),
    hardLimit: LIMIT,
    scope: "account",
    name: "User quota",
    types: ["Email"],
    ...extra,
  };
}

async function withQuotas(
  page: Page,
  quotas: Record<string, unknown>[] | undefined,
): Promise<MockedMail> {
  return openInbox(page, "right", INBOX_PATH, { mail: quotas === undefined ? {} : { quotas } });
}

function sidebar(page: Page) {
  return page.getByRole("navigation", { name: "Mailboxes and accounts" });
}

test("the sidebar's foot shows the quota as a meter; its hint names the figures on hover and on focus", async ({
  page,
}) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await withQuotas(page, [storage(4.2 * GB)]);
  const meter = sidebar(page).getByRole("meter", { name: PLAIN });
  await expect(meter).toContainText("4.2 of 10 GB");
  await expect(meter).toHaveAttribute("aria-valuetext", "42%");
  await settled(page);
  for (const theme of THEMES) {
    await page.emulateMedia({ colorScheme: theme });
    await expect.soft(sidebar(page)).toHaveScreenshot(`quota-foot-${theme}-desktop.png`);
  }
  expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
  const hint = page.getByText(PLAIN);
  await meter.hover();
  await expect(hint).toBeVisible();
  await page.mouse.move(0, 0);
  await expect(hint).toBeHidden();
  await sidebar(page).getByRole("treeitem").first().focus();
  await page.keyboard.press("Tab");
  await expect(meter).toBeFocused();
  await expect(hint).toBeVisible();
  expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
});

test("past 90% the bar warns with its word", async ({ page }) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await withQuotas(page, [storage(9.1 * GB)]);
  const meter = sidebar(page).getByRole("meter", { name: NEARLY_FULL });
  await expect(meter).toContainText("Almost full");
  await expect(meter).toHaveAttribute("aria-valuetext", "Almost full");
  await settled(page);
  await expect.soft(sidebar(page)).toHaveScreenshot("quota-warn-light-desktop.png");
});

test("a server's own warn limit warns before 90%", async ({ page }) => {
  await withQuotas(page, [storage(8 * GB, { warnLimit: 7.5 * GB })]);
  await expect(sidebar(page).getByRole("meter")).toContainText("Almost full");
});

test("a server that counts messages alone reads in messages", async ({ page }) => {
  const messages = {
    ...storage(1204),
    id: "q-count",
    resourceType: "count",
    hardLimit: 10_000,
  };
  await withQuotas(page, [messages]);
  const meter = sidebar(page).getByRole("meter", {
    name: "Mail storage: 1,204 of 10,000 messages",
  });
  await expect(meter).toContainText("1,204 of 10,000 messages");
});

test("an account without the quota capability shows no bar", async ({ page }) => {
  await withQuotas(page, undefined);
  await expect(sidebar(page).getByRole("treeitem").first()).toBeVisible();
  await expect(page.getByRole("meter")).toHaveCount(0);
});

test("quotas of another scope or another type show no bar and no error", async ({ page }) => {
  const asked = page
    .context()
    .waitForEvent("request", (request) => request.postData()?.includes("Quota/get") === true);
  await withQuotas(page, [storage(GB, { scope: "domain" }), storage(GB, { types: ["Calendar"] })]);
  await asked;
  await expect(page.getByRole("meter")).toHaveCount(0);
  await expect(sidebar(page)).not.toContainText("Couldn’t");
});

test("the rail shows the bar alone; its hint names the figures", async ({ page }) => {
  await page.setViewportSize(TABLET);
  await withQuotas(page, [storage(9.1 * GB)]);
  const meter = sidebar(page).getByRole("meter", { name: NEARLY_FULL });
  await expect(meter).not.toContainText("of 10 GB");
  await settled(page);
  await expect.soft(sidebar(page)).toHaveScreenshot("quota-rail-light-tablet.png");
  await meter.hover();
  await expect(page.getByText(NEARLY_FULL)).toBeVisible();
});

test("a phone's sheet carries the full foot at its end", async ({ page }) => {
  await page.setViewportSize(VIEWPORTS[0]);
  await withQuotas(page, [storage(9.1 * GB)]);
  await page.getByRole("button", { name: "Mailboxes and accounts" }).click();
  const sheet = page.getByRole("dialog", { name: "Mailboxes and accounts" });
  const meter = sheet.getByRole("meter", { name: NEARLY_FULL });
  await expect(meter).toContainText("9.1 of 10 GB");
  // The sheet slides in; a trial click waits until the bar stands still.
  await meter.click({ trial: true });
  await settled(page);
  await expect.soft(sheet).toHaveScreenshot("quota-sheet-light-phone.png");
  expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
});

test("the foot reads right to left in the pseudo-locale", async ({ page }) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await withQuotas(page, [storage(9.1 * GB)]);
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "en-XA");
  });
  await page.goto(INBOX_PATH);
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
  // The sidebar's name reads in the pseudo-locale too.
  const pseudoSidebar = page.getByRole("navigation");
  await expect(pseudoSidebar.getByRole("meter")).toContainText("Álmóst fúll");
  await settled(page);
  await expect.soft(pseudoSidebar).toHaveScreenshot("quota-foot-en-XA-light-desktop.png");
});
