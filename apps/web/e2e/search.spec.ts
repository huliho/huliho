// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Locator, Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { accountRow, mockAccounts } from "./account-mocks";
import { FIXED_NOW } from "./mail-corpus";
import { mockMail } from "./mail-mocks";
import { mockPreferences } from "./preference-mocks";
import { mockSignedIn } from "./session-mocks";
import { axeOn } from "./sweep";
import { settled } from "./thread-pane";

// The platform's command modifier, as Playwright names it.
const MOD = "ControlOrMeta";
const PSEUDO_LOCALE = "en-XA";
const PHONE = { width: 390, height: 844 };

async function openInbox(page: Page): Promise<void> {
  await mockSignedIn(page);
  await mockMail(page);
  await mockAccounts(page, [accountRow(FIXED_NOW)]);
  await page.clock.setFixedTime(FIXED_NOW);
  await page.goto("/mail/acc-1/mb-inbox");
  await expect(page.getByRole("grid").locator('[role="row"]').first()).toBeVisible();
}

function field(page: Page): Locator {
  return page.getByRole("combobox", { name: "Search mail" });
}

function spoken(page: Page): Locator {
  return page.getByRole("search").getByRole("status");
}

// The direction an element is laid out in, as the browser resolved it.
function directionOf(element: Element): string {
  return getComputedStyle(element).direction;
}

test("`/` from the list opens the search with the focus in the field and Escape hands it back", async ({
  page,
}) => {
  await openInbox(page);
  await page.locator('[role="grid"] [role="row"][tabindex="0"]').focus();
  await page.keyboard.press("/");
  await expect(page).toHaveURL(/\/mail\/acc-1\/search\?box=mb-inbox$/u);
  await expect(field(page)).toBeFocused();
  await expect(page.getByRole("button", { name: "from: a sender" })).toBeVisible();
  expect((await axeOn(page).analyze()).violations).toEqual([]);
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-inbox$/u);
  await expect(page.locator('[role="grid"] [role="row"]:focus')).toHaveCount(1);
});

test("typing offers the plain search and the senders; Enter runs and the slot counts", async ({
  page,
}) => {
  await openInbox(page);
  await page.getByRole("button", { name: "Search mail" }).click();
  await field(page).pressSequentially("offerte");
  await expect(page.getByRole("option", { name: "Search for “offerte”" })).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/q=offerte/u);
  await expect(field(page)).toHaveValue("offerte");
  await expect(spoken(page)).toHaveText(/^[\d,]+ results? for “offerte” in all mailboxes$/u);
  expect((await axeOn(page).analyze()).violations).toEqual([]);
});

test("a press on the widening lands while the field still holds the focus", async ({ page }) => {
  await openInbox(page);
  await page.goto("/mail/acc-1/search?q=offerte&box=mb-inbox");
  await expect(field(page)).toBeFocused();
  await expect(page.getByRole("button", { name: "from: a sender" })).toBeVisible();
  await page.getByRole("button", { name: "Include Junk and Trash" }).click();
  await expect(page).toHaveURL(/in=all/u);
  await expect(spoken(page)).toHaveText(/Junk and Trash included$/u);
});

test("Down walks to a sender and Enter runs it in place of the text", async ({ page }) => {
  await openInbox(page);
  await page.getByRole("button", { name: "Search mail" }).click();
  await field(page).pressSequentially("fem");
  const sender = page.getByRole("option", { name: "from: Femke Aalders, femke@kastanje.example" });
  await expect(sender).toBeVisible();
  await page.keyboard.press("ArrowDown");
  await expect(sender).toHaveAttribute("data-highlighted");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\?from=femke%40kastanje\.example&box=mb-inbox$/u);
  const recents = await page.evaluate(() => localStorage.getItem("huliho-recent-searches:acc-1"));
  expect(JSON.parse(recents ?? "[]")).toEqual(['{"from":"femke@kastanje.example"}']);
});

test("a sender picked from the suggestions becomes a token and runs at once", async ({ page }) => {
  await openInbox(page);
  await page.keyboard.press(`${MOD}+k`);
  await page.getByRole("option", { name: /^Search mail/u }).click();
  await field(page).pressSequentially("from:fem");
  await page.getByRole("option", { name: "from: Femke Aalders, femke@kastanje.example" }).click();
  await expect(page.getByRole("button", { name: "Remove from: Femke Aalders" })).toBeVisible();
  await expect(page).toHaveURL(/from=femke%40kastanje\.example/u);
  await expect(spoken(page)).toHaveText(/^[\d,]+ results? in all mailboxes$/u);
});

test("the field, a token and the suggestions read in Dutch and mirror in the pseudo-locale", async ({
  page,
}) => {
  await mockSignedIn(page);
  await mockMail(page);
  await mockAccounts(page, [accountRow(FIXED_NOW)]);
  await mockPreferences(page, { locale: "nl" });
  await page.clock.setFixedTime(FIXED_NOW);
  await page.goto("/mail/acc-1/search?from=femke%40kastanje.example&box=mb-inbox");
  const dutch = page.getByRole("combobox", { name: "Zoeken in mail" });
  // No mail is held yet, so the token names the sender by the address.
  await expect(
    page.getByRole("button", { name: "from: femke@kastanje.example verwijderen" }),
  ).toBeVisible();
  await dutch.pressSequentially("off");
  await expect(page.getByRole("option", { name: "Zoeken naar “off”" })).toBeVisible();
  await settled(page);
  await expect.soft(page).toHaveScreenshot("search-nl-light-desktop.png");
  // The pseudo-locale lives on the device alone and never reaches the server.
  await page.addInitScript((locale) => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", locale);
  }, PSEUDO_LOCALE);
  await page.goto("/mail/acc-1/search?from=femke%40kastanje.example&box=mb-inbox");
  const mirrored = page.getByRole("search");
  await expect(mirrored).toBeVisible();
  expect(await mirrored.evaluate(directionOf)).toBe("rtl");
  await page.getByRole("combobox").pressSequentially("off");
  const mailbox = page.getByRole("option", { name: "in: Offertes" });
  await expect(mailbox).toBeVisible();
  // The operator words stay English and the typed text is Latin, so both read left to right.
  // The token's name holds the operator word as is, before the sender however it reads.
  const token = mirrored.getByRole("button", { name: /from: /u });
  const shortcut = page.getByRole("button", { name: "fróm: á séñdér" });
  expect(await token.getByText("from:", { exact: true }).evaluate(directionOf)).toBe("ltr");
  expect(await mailbox.getByText("in:", { exact: true }).evaluate(directionOf)).toBe("ltr");
  expect(await shortcut.evaluate(directionOf)).toBe("ltr");
  expect(await page.getByRole("combobox").evaluate(directionOf)).toBe("ltr");
  await settled(page);
  await expect.soft(page).toHaveScreenshot(`search-${PSEUDO_LOCALE}-light-desktop.png`);
  // Escape clears the field with its token; the route's search then stands under it as a recent.
  await page.keyboard.press("Escape");
  const recent = page.getByRole("option", {
    name: /^from: (?:Femke Aalders|femke@kastanje\.example)$/u,
  });
  await expect(recent).toBeVisible();
  expect(await recent.getByText("from:", { exact: true }).evaluate(directionOf)).toBe("ltr");
});

test("on a phone the shortcut writes its operator and the field keeps the keyboard", async ({
  page,
}) => {
  await page.setViewportSize(PHONE);
  await openInbox(page);
  await page.getByRole("button", { name: "Search mail" }).click();
  await page.getByRole("button", { name: "from: a sender" }).click();
  await expect(field(page)).toHaveValue("from:");
  await expect(field(page)).toBeFocused();
  await expect(page.getByRole("option", { name: /^from: / }).first()).toBeVisible();
});
