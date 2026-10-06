// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Locator, Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { mockAccounts } from "./account-mocks";
import { NOT_AN_IMAGE_NAME } from "./blob-mocks";
import {
  ATTACHMENTS_SENDER,
  DANGEROUS_IMAGE_NAME,
  DANGEROUS_NAME,
  LONG_NAME,
  PHOTO_NAME,
  SVG_NAME,
} from "./mail-bodies";
import { FIXED_NOW } from "./mail-corpus";
import { mockMail } from "./mail-mocks";
import { mockPreferences } from "./preference-mocks";
import { mockSignedIn } from "./session-mocks";
import { THEMES, VIEWPORTS, axeOn } from "./sweep";
import {
  READ_INBOX,
  ROWS,
  cards,
  openInbox,
  pathOf,
  rowFrom,
  screenOf,
  settled,
} from "./thread-pane";

// The planning mail carries this many parts.
const ATTACHMENT_COUNT = 8;
const LIST_NAME = `${String(ATTACHMENT_COUNT)} attachments`;
const WARNING = "This file can run programs on your computer.";
const PHOTO_BUTTON = `${PHOTO_NAME} 2.4 MB`;
const DANGEROUS_BUTTON = `${DANGEROUS_NAME} 36 kB`;
// The photo the mocked route serves is this wide.
const PHOTO_WIDTH = 480;
// The narrowest screen the layout holds.
const NARROW = { width: 320, height: 640 };
// Long enough for a download to have started if anything had asked for one.
const QUIET_MS = 500;

// The open card of the planning mail and its strip.
async function openStrip(page: Page): Promise<{ card: Locator; strip: Locator }> {
  await openInbox(page, "right", pathOf(rowFrom(ATTACHMENTS_SENDER)));
  const card = cards(page).last();
  const strip = card.getByRole("list", { name: LIST_NAME });
  await expect(strip).toBeVisible();
  return { card, strip };
}

// Whether the element is an image that decoded as a picture.
function loaded(image: Locator): Promise<boolean> {
  return image.evaluate(
    (element) =>
      element instanceof HTMLImageElement && element.complete && element.naturalWidth > 0,
  );
}

function naturalWidth(image: Locator): Promise<number> {
  return image.evaluate((element) =>
    element instanceof HTMLImageElement ? element.naturalWidth : 0,
  );
}

// The names of the downloads the page started, each canceled at once.
function watchDownloads(page: Page): string[] {
  const names: string[] = [];
  page.on("download", (download) => {
    names.push(download.suggestedFilename());
    void download.cancel();
  });
  return names;
}

test("the strip lists the chips first and the preview after them, each named and sized; a part the server turned into a download is a chip", async ({
  page,
}) => {
  const { strip } = await openStrip(page);
  const items = strip.getByRole("listitem");
  await expect(items).toHaveCount(ATTACHMENT_COUNT);
  const pdf = strip.getByRole("link", { name: `${LONG_NAME} 48 kB` });
  await expect(pdf).toHaveAttribute(
    "href",
    /\/download\/u1\/[^/]+\/Offerte_badkamer_renovatie_v3_definitief\.pdf\?type=application%2Foctet-stream$/,
  );
  await expect(pdf).toHaveAttribute("download", "");
  await expect(strip.getByRole("link", { name: "Unnamed attachment 1 kB" })).toBeVisible();
  await expect(strip.getByRole("link", { name: "Attached message 12 kB" })).toHaveAttribute(
    "href",
    /\/message\.eml\?type=application%2Foctet-stream$/,
  );
  // The file that can run a program is a button, so nothing downloads on its own.
  await expect(strip.getByRole("button", { name: DANGEROUS_BUTTON })).toBeVisible();
  // The photo previews last, from the route with its declared type.
  const preview = items.last().getByRole("button", { name: PHOTO_BUTTON });
  await expect(preview).toBeVisible();
  await expect(preview.locator("img")).toHaveAttribute("src", /type=image%2Fpng$/);
  await expect.poll(() => loaded(preview.locator("img"))).toBe(true);
  expect(await naturalWidth(preview.locator("img"))).toBe(PHOTO_WIDTH);
  await expect(strip.getByRole("link", { name: `${NOT_AN_IMAGE_NAME} 512 kB` })).toBeVisible();
  await expect(strip.getByRole("button", { name: /scan\.png/ })).toHaveCount(0);
  // An image under a name that asks first never previews.
  await expect(strip.getByRole("button", { name: `${DANGEROUS_IMAGE_NAME} 20 kB` })).toBeVisible();
  await expect(strip.locator("img")).toHaveCount(1);
});

test("a photo opens at full size with its name, its size and Download; Escape and Close return the focus to the preview", async ({
  page,
}) => {
  const { strip } = await openStrip(page);
  const preview = strip.getByRole("button", { name: PHOTO_BUTTON });
  await preview.click();
  const dialog = page.getByRole("dialog", { name: PHOTO_NAME });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText("2.4 MB")).toBeVisible();
  await expect(dialog.getByRole("link", { name: "Download" })).toHaveAttribute(
    "href",
    /\/tegelwerk_voorbeeld\.png\?type=application%2Foctet-stream$/,
  );
  const picture = dialog.getByRole("img", { name: PHOTO_NAME });
  await expect.poll(() => loaded(picture)).toBe(true);
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(preview).toBeFocused();
  await preview.click();
  await page.getByRole("dialog").getByRole("button", { name: "Close" }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(preview).toBeFocused();
});

test("a name that can run a program asks first: Cancel has the focus and downloads nothing, Download anyway downloads, the chip takes the focus back either way", async ({
  page,
}) => {
  const { strip } = await openStrip(page);
  const downloads = watchDownloads(page);
  const chip = strip.getByRole("button", { name: DANGEROUS_BUTTON });
  await chip.click();
  const dialog = page.getByRole("dialog", { name: WARNING });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText(`${DANGEROUS_NAME} · 36 kB`)).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(chip).toBeFocused();
  await page.waitForTimeout(QUIET_MS);
  expect(downloads).toEqual([]);
  await chip.click();
  const saving = page.waitForEvent("download");
  await page.getByRole("dialog").getByRole("link", { name: "Download anyway" }).click();
  // The name the browser saves under comes from the route's answer, which
  // no mocked route reaches here, so the address asked is what holds.
  expect((await saving).url()).toMatch(/\/factuur_viewer\.html\?type=application%2Foctet-stream$/);
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(chip).toBeFocused();
});

test("an SVG never previews: it is a chip that asks first because it can carry a script and then downloads under its name", async ({
  page,
}) => {
  const { strip } = await openStrip(page);
  await expect(strip.getByRole("link", { name: /diagram\.svg/ })).toHaveCount(0);
  await strip.getByRole("button", { name: `${SVG_NAME} 8 kB` }).click();
  const dialog = page.getByRole("dialog", { name: WARNING });
  await expect(dialog.getByText(`${SVG_NAME} · 8 kB`)).toBeVisible();
  const saving = page.waitForEvent("download");
  await dialog.getByRole("link", { name: "Download anyway" }).click();
  const saved = await saving;
  expect(saved.url()).toMatch(/\/diagram\.svg\?type=application%2Foctet-stream$/);
  await saved.cancel();
});

// How far the page or the thread's own screen scrolls sideways.
async function overflowOf(page: Page): Promise<number> {
  const region = await screenOf(page).evaluate(
    (element) => element.scrollWidth - element.clientWidth,
  );
  const root = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  return Math.max(region, root);
}

test("at the phone floor every chip stands on a row of its own, a long name gives way in the middle and nothing scrolls sideways", async ({
  page,
}) => {
  await page.setViewportSize(NARROW);
  const { strip } = await openStrip(page);
  const tops = await Promise.all(
    (await strip.getByRole("link").all()).map(async (chip) => (await chip.boundingBox())?.y ?? 0),
  );
  expect(new Set(tops).size).toBe(tops.length);
  const pdf = strip.getByRole("link", { name: `${LONG_NAME} 48 kB` });
  const start = pdf.getByText("Offerte_badkamer_renovatie_v3_", { exact: true });
  const end = pdf.getByText("definitief.pdf", { exact: true });
  expect(await start.evaluate((element) => element.scrollWidth > element.clientWidth)).toBe(true);
  expect(await end.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
  expect(await overflowOf(page)).toBe(0);
});

test("the strip reads in Dutch and in the pseudo-locale", async ({ page }) => {
  const [phone] = VIEWPORTS;
  await page.setViewportSize({ width: phone.width, height: phone.height });
  const row = rowFrom(ATTACHMENTS_SENDER);
  await mockSignedIn(page);
  await mockMail(page, READ_INBOX);
  await mockAccounts(page, ROWS);
  await mockPreferences(page, { locale: "nl" });
  await page.clock.setFixedTime(FIXED_NOW);
  await page.goto(pathOf(row));
  const strip = cards(page)
    .last()
    .getByRole("list", { name: `${String(ATTACHMENT_COUNT)} bijlagen` });
  await expect(strip.getByRole("link", { name: "Bijlage zonder naam 1 kB" })).toBeVisible();
  await expect(strip.getByRole("link", { name: "Bijgevoegd bericht 12 kB" })).toBeVisible();
  await expect.poll(() => loaded(strip.locator("img"))).toBe(true);
  await settled(page);
  await expect.soft(page).toHaveScreenshot("attachments-nl-light-phone.png");
  await strip.getByRole("button", { name: DANGEROUS_BUTTON }).click();
  const dialog = page.getByRole("dialog", {
    name: "Dit bestand kan programma's op je computer uitvoeren.",
  });
  await expect(dialog.getByRole("button", { name: "Annuleren" })).toBeFocused();
  await expect(dialog.getByRole("link", { name: "Toch downloaden" })).toBeVisible();
  await page.keyboard.press("Escape");
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "en-XA");
  });
  await page.goto(pathOf(row));
  const pseudo = cards(page)
    .last()
    .getByRole("list", { name: /áttáçhméñts/ });
  await expect(pseudo.getByRole("link", { name: /Úññáméd áttáçhméñt/ })).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
  // A file name is mail content and keeps its own direction inside the right-to-left page.
  const name = pseudo.getByText("definitief.pdf", { exact: true });
  expect(await name.evaluate((element) => getComputedStyle(element).direction)).toBe("ltr");
  // A size reads its digits before its unit, whatever the page's direction.
  const size = pseudo
    .getByRole("link", { name: `${LONG_NAME} 48 kB` })
    .getByText("48 kB", { exact: true });
  expect(await size.evaluate((element) => getComputedStyle(element).direction)).toBe("ltr");
  await expect.poll(() => loaded(pseudo.locator("img"))).toBe(true);
  await settled(page);
  await expect.soft(page).toHaveScreenshot("attachments-en-XA-light-phone.png");
  // The question names the file the same way.
  await pseudo.getByRole("button", { name: DANGEROUS_BUTTON }).click();
  const asked = page.getByRole("dialog").getByText(DANGEROUS_NAME, { exact: true });
  expect(await asked.evaluate((element) => getComputedStyle(element).direction)).toBe("ltr");
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("the strip and its dialogs are axe-clean and match their screenshots in both themes at both widths", async ({
  page,
}) => {
  for (const viewport of VIEWPORTS) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    for (const theme of THEMES) {
      await page.emulateMedia({ colorScheme: theme });
      const label = `the strip in ${theme} at ${viewport.name} width`;
      await test.step(label, async () => {
        const { strip } = await openStrip(page);
        const preview = strip.getByRole("button", { name: PHOTO_BUTTON });
        await expect.poll(() => loaded(preview.locator("img"))).toBe(true);
        await settled(page);
        expect.soft((await axeOn(page).analyze()).violations, `axe on ${label}`).toEqual([]);
        await expect.soft(page).toHaveScreenshot(`attachments-${theme}-${viewport.name}.png`);
        if (theme !== "light") {
          return;
        }
        await preview.click();
        const picture = page.getByRole("dialog").getByRole("img", { name: PHOTO_NAME });
        await expect.poll(() => loaded(picture)).toBe(true);
        await settled(page);
        expect.soft((await axeOn(page).analyze()).violations, `axe on the preview`).toEqual([]);
        await expect.soft(page).toHaveScreenshot(`attachments-preview-${viewport.name}.png`);
        await page.keyboard.press("Escape");
        await strip.getByRole("button", { name: DANGEROUS_BUTTON }).click();
        await expect(page.getByRole("dialog", { name: WARNING })).toBeVisible();
        await settled(page);
        expect.soft((await axeOn(page).analyze()).violations, `axe on the warning`).toEqual([]);
        await expect.soft(page).toHaveScreenshot(`attachments-warning-${viewport.name}.png`);
        await page.keyboard.press("Escape");
      });
    }
  }
});
