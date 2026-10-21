// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import type { MailServer } from "./mail-answers";
import { THEMES, VIEWPORTS, axeOn } from "./sweep";
import {
  INBOX_PATH,
  WRITABLE_READ_INBOX,
  openInbox,
  pane,
  rowAt,
  settled,
  subjectOf,
  threadOf,
} from "./thread-pane";

// Every mailbox a move from the inbox may enter, in the tree's order.
const TARGETS = ["Drafts", "Sent", "Archive", "Facturen", "Verbouwing", "Verbouwing / Offertes"];

function picker(page: Page) {
  return page.getByRole("dialog", { name: "Move to…" });
}

function toast(page: Page, text: string) {
  return page.getByRole("dialog").filter({ hasText: text });
}

const MAILBOX_PATH = "mailboxIds/";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

// The mailboxes one patch adds.
function addedBy(patch: unknown): string[] {
  return isRecord(patch)
    ? Object.entries(patch).flatMap(([path, value]) =>
        path.startsWith(MAILBOX_PATH) && value === true ? [path.slice(MAILBOX_PATH.length)] : [],
      )
    : [];
}

// The mailboxes each Email/set moved into, by the paths its patches set.
function entered(server: MailServer): string[] {
  return server.sets.flatMap(({ update }) =>
    isRecord(update) ? Object.values(update).flatMap(addedBy) : [],
  );
}

async function writable(page: Page) {
  return openInbox(page, "right", INBOX_PATH, { mailboxes: WRITABLE_READ_INBOX });
}

test("m opens the picker; typing narrows it, Enter moves the cursor's thread and the next m holds it under Recent", async ({
  page,
}) => {
  const { server } = await writable(page);
  await rowAt(page, 1).focus();
  await page.keyboard.press("m");
  await expect(picker(page).getByRole("option")).toHaveText(TARGETS);
  await expect(picker(page).getByRole("combobox")).toBeFocused();
  await page.keyboard.type("fac");
  await expect(picker(page).getByRole("option")).toHaveText(["Facturen"]);
  await page.keyboard.press("Enter");
  await expect(toast(page, "Moved to Facturen.")).toBeVisible();
  await expect(rowAt(page, 1)).toContainText(subjectOf(2));
  // The write follows the log's short wait behind the toast.
  await expect.poll(() => entered(server)).toHaveLength(threadOf(1).length);
  expect(new Set(entered(server))).toEqual(new Set(["mb-facturen"]));
  await page.keyboard.press("m");
  await expect(picker(page).getByText("Recent")).toBeVisible();
  await expect(picker(page).getByRole("option")).toHaveText(["Facturen", ...TARGETS]);
  expect(await axeOn(page).analyze()).toMatchObject({ violations: [] });
  await settled(page);
  for (const theme of THEMES) {
    await page.emulateMedia({ colorScheme: theme });
    await expect.soft(picker(page)).toHaveScreenshot(`picker-${theme}-desktop.png`);
  }
  await page.keyboard.type("zz");
  await expect(picker(page).getByRole("status")).toHaveText("No mailbox matches.");
  await page.keyboard.press("Escape");
  await expect(picker(page)).toBeHidden();
  await expect(rowAt(page, 1)).toBeFocused();
});

test("a key pressed before the picker holds the focus runs nothing on the page", async ({
  page,
}) => {
  const { server } = await writable(page);
  await rowAt(page, 1).focus();
  // Both keys land in one task, ahead of the frame in which the picker takes the focus.
  await page.evaluate(() => {
    for (const key of ["m", "e"]) {
      document.activeElement?.dispatchEvent(
        new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }),
      );
    }
  });
  await expect(picker(page).getByRole("combobox")).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(rowAt(page, 1)).toBeFocused();
  await expect(rowAt(page, 1)).toContainText(subjectOf(1));
  await expect(toast(page, "Archived.")).toHaveCount(0);
  expect(server.sets).toEqual([]);
});

test("the toolbar's Move to… takes the open thread", async ({ page }) => {
  const { server } = await writable(page);
  await rowAt(page, 2).focus();
  await page.keyboard.press("Enter");
  await pane(page).getByRole("button", { name: "Move to…", exact: true }).click();
  await picker(page).getByRole("option", { name: "Archive" }).click();
  await expect(toast(page, "Moved to Archive.")).toBeVisible();
  await expect.poll(() => entered(server)).toHaveLength(threadOf(2).length);
  expect(new Set(entered(server))).toEqual(new Set(["mb-archive"]));
});

test("the picker reads right to left in the pseudo-locale, with Recent and a nested path", async ({
  page,
}) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await writable(page);
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "en-XA");
  });
  await page.goto(INBOX_PATH);
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
  const first = page.getByRole("grid").locator('[aria-rowindex="1"]');
  await first.focus();
  await page.keyboard.press("m");
  const pseudo = page.getByRole("dialog", { name: /Móvé tó/ });
  await expect(pseudo.getByRole("combobox")).toBeFocused();
  await page.keyboard.type("fac");
  await expect(pseudo.getByRole("option")).toHaveText(["Facturen"]);
  await page.keyboard.press("Enter");
  await expect(first).toContainText(subjectOf(2));
  await page.keyboard.press("m");
  await expect(pseudo.getByRole("option")).toHaveText(["Facturen", ...TARGETS]);
  await settled(page);
  await expect.soft(pseudo).toHaveScreenshot("picker-en-XA-light-desktop.png");
});

test.describe("on a phone", () => {
  test.use({ viewport: VIEWPORTS[0] });

  test("the picker hangs from the top with the tree's mailboxes", async ({ page }) => {
    await writable(page);
    await rowAt(page, 1).focus();
    await page.keyboard.press("m");
    await expect(picker(page).getByRole("option")).toHaveText(TARGETS);
    await settled(page);
    await expect.soft(page).toHaveScreenshot("picker-light-phone.png");
  });
});
