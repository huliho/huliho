// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Locator, Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { mockAccounts } from "./account-mocks";
import { MESSAGE } from "./blob-mocks";
import { releaseBodies } from "./mail-answers";
import {
  AUTHSERV,
  FAILED_SENDER,
  MICROSOFT_SENDER,
  NEWSLETTER_SENDER,
  REPLY_SENDER,
  UNCHECKED_SENDER,
  UNREADABLE_SENDER,
} from "./mail-bodies";
import { FIXED_NOW } from "./mail-corpus";
import { mockMail } from "./mail-mocks";
import type { MockedMail } from "./mail-mocks";
import { network } from "./network";
import { mockPreferences } from "./preference-mocks";
import { mockSignedIn } from "./session-mocks";
import { THEMES, VIEWPORTS, axeOn, framesLoaded } from "./sweep";
import {
  INBOX_PATH,
  READ_INBOX,
  ROWS,
  cards,
  directionOf,
  newestOf,
  openInbox,
  pathOf,
  rowAt,
  rowFrom,
  settled,
} from "./thread-pane";

const DETAILS = "Message details";
const TABS = ["Rendered", "Plain text", "Source"];
const NO_PLAIN = "No plain-text version of this message.";
const CUT = "Showing the first 512 kB of this message.";
const LOADING = "Loading…";
const FAILED = "Couldn’t load this message. Your mail is safe; nothing was lost.";
const OFFLINE = "Offline: this message isn’t stored on this device.";
// The first bytes the source view asks for.
const SOURCE_RANGE = "bytes=0-524287";
const SOURCE_CAP = 524_288;
// The platform's command modifier, as Playwright names it.
const MOD = "ControlOrMeta";
// The tabs at phone width share the row within this many pixels of each other.
const SHARE_TOLERANCE_PX = 2;

function dialog(page: Page): Locator {
  return page.getByRole("dialog", { name: DETAILS });
}

function lineOf(page: Page): Locator {
  return dialog(page).getByText(/Checked by/);
}

// The open card of the row's thread with its details button.
async function openCard(
  page: Page,
  rowIndex: number,
): Promise<{ card: Locator; mail: MockedMail }> {
  const mail = await openInbox(page, "right", pathOf(rowIndex));
  const card = cards(page).last();
  await expect(card.getByRole("button").first()).toHaveAttribute("aria-expanded", "true");
  return { card, mail };
}

async function openDetails(page: Page, card: Locator): Promise<Locator> {
  await card.getByRole("button", { name: DETAILS }).click();
  await expect(dialog(page)).toBeVisible();
  return dialog(page);
}

// Shows the tab and answers its panel once the panel it replaces has
// left, which takes the engine a frame.
async function showTab(details: Locator, name: string): Promise<Locator> {
  await details.getByRole("tab", { name }).click();
  const panel = details.getByRole("tabpanel");
  await expect(panel).toHaveCount(1);
  return panel;
}

test("the details open from the head with the receiving server's line, the three tabs and the download; Escape closes them and the button takes the focus back", async ({
  page,
}) => {
  const row = rowFrom(NEWSLETTER_SENDER);
  const { card, mail } = await openCard(page, row);
  const details = await openDetails(page, card);
  await expect(lineOf(page)).toHaveText(
    `Checked by ${AUTHSERV}: SPF passed, DKIM passed, DMARC passed.`,
  );
  await expect(details.getByRole("tab")).toHaveText(TABS);
  await expect(details.getByRole("button", { name: "Close" })).toBeFocused();
  const frame = details.locator("iframe");
  await expect(frame).toHaveAttribute(
    "sandbox",
    "allow-same-origin allow-popups allow-popups-to-escape-sandbox",
  );
  await framesLoaded(page);
  await expect(details.frameLocator("iframe").getByRole("heading", { level: 1 })).toHaveText(
    "Week 35: rentes, chips en de bouw",
  );
  // The newsletter carries no text part.
  await expect(await showTab(details, "Plain text")).toHaveText(NO_PLAIN);
  expect(mail.blobs.ranges).toEqual([]);
  await showTab(details, "Source");
  const source = details.locator("pre");
  await expect(source).toHaveText(MESSAGE.replaceAll("\r\n", "\n"));
  expect(await directionOf(source)).toBe("ltr");
  expect(mail.blobs.ranges).toEqual([SOURCE_RANGE]);
  await expect(details.getByText(CUT)).toHaveCount(0);
  const newest = newestOf(row);
  await expect(details.getByRole("link", { name: "Download the message" })).toHaveAttribute(
    "href",
    `/api/jmap/acc-1/download/u1/${newest?.id ?? ""}/message.eml?type=message%2Frfc822`,
  );
  // With the focus inside the message's frame the keys reach the
  // registry and not the dialog: a list key and the palette's chord
  // stay with the dialog, Escape closes it.
  await showTab(details, "Rendered");
  await framesLoaded(page);
  const link = details.frameLocator("iframe").getByRole("link").first();
  await link.focus();
  await page.keyboard.press("j");
  await page.keyboard.press(`${MOD}+k`);
  await expect(link).toBeFocused();
  await expect(page.getByRole("dialog", { name: "Command palette" })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(card.getByRole("button", { name: DETAILS })).toBeFocused();
  // The thread stays open behind it.
  await expect(card.getByRole("button").first()).toHaveAttribute("aria-expanded", "true");
});

// One test per width: a body the device holds from an earlier visit
// never asks the server again, so each run starts with a fresh device.
for (const viewport of VIEWPORTS) {
  test(`at ${viewport.name} width the details on their way show the still lines until the body lands; a source the server refuses offers Try again and one this device cannot reach says so`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    const row = rowFrom(NEWSLETTER_SENDER);
    const mail = await openInbox(page, "right", INBOX_PATH);
    const connect = await network(page);
    mail.server.holdBodies = true;
    await rowAt(page, row).click();
    const card = cards(page).last();
    const details = await openDetails(page, card);
    await expect(details.getByRole("status", { name: LOADING })).toBeVisible();
    await expect(lineOf(page)).toHaveCount(0);
    await expect(details.getByRole("link", { name: "Download the message" })).toHaveCount(0);
    await settled(page);
    await expect.soft(page).toHaveScreenshot(`inspector-loading-light-${viewport.name}.png`);
    releaseBodies(mail.server);
    await expect(lineOf(page)).toContainText(`Checked by ${AUTHSERV}`);
    await expect(details.getByRole("link", { name: "Download the message" })).toBeVisible();
    // The source is never on the device, so offline it says so.
    await connect(false);
    const source = await showTab(details, "Source");
    await expect(source.getByText(OFFLINE)).toBeVisible();
    await expect(source.getByRole("button", { name: "Try again" })).toHaveCount(0);
    expect(mail.blobs.ranges).toEqual([]);
    await settled(page);
    await expect.soft(page).toHaveScreenshot(`inspector-offline-light-${viewport.name}.png`);
    await connect(true);
    // The Source tab stays shown: the read asks again by itself once the network returns.
    await expect(source.locator("pre")).toContainText("From: sender@example.test");
    expect(mail.blobs.ranges).toEqual([SOURCE_RANGE]);
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog")).toHaveCount(0);
    // A source the route refuses, as a proxy whose upstream is down answers.
    const refused = newestOf(rowFrom(REPLY_SENDER))?.id ?? "";
    mail.blobs.refusedMessages.add(refused);
    const retried = await openDetails(page, await openThreadOf(page, REPLY_SENDER));
    const failed = await showTab(retried, "Source");
    await expect(failed.getByRole("alert")).toContainText(FAILED);
    await settled(page);
    await expect.soft(page).toHaveScreenshot(`inspector-failed-light-${viewport.name}.png`);
    mail.blobs.refusedMessages.delete(refused);
    await failed.getByRole("button", { name: "Try again" }).click();
    await expect(retried.locator("pre")).toContainText("From: sender@example.test");
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog")).toHaveCount(0);
  });
}

test("a message longer than the source view reads shows its first part with the cut notice under it; the plain tab shows the text as the card does", async ({
  page,
}) => {
  const row = rowFrom(REPLY_SENDER);
  const { card, mail } = await openCard(page, row);
  mail.blobs.longMessages.add(newestOf(row)?.id ?? "");
  const details = await openDetails(page, card);
  await showTab(details, "Source");
  await expect(details.getByText(CUT)).toBeVisible();
  const source = details.locator("pre");
  await expect(source).toContainText("From: sender@example.test");
  expect((await source.textContent())?.length).toBe(SOURCE_CAP);
  const plain = await showTab(details, "Plain text");
  const quoted = plain.getByText(/Kunnen jullie een offerte maken/);
  expect(
    await quoted.evaluate((element) => element.closest("[data-depth]")?.getAttribute("data-depth")),
  ).toBe("2");
});

// The newest card of the sender's thread, reached by its address once
// the tab is signed in, so no row has to be in sight.
async function openThreadOf(page: Page, sender: string): Promise<Locator> {
  await page.goto(pathOf(rowFrom(sender)));
  const card = cards(page).last();
  await expect(card.getByRole("button").first()).toHaveAttribute("aria-expanded", "true");
  return card;
}

test("the line follows the header: the reader's own server for the Microsoft shape, a failed check, unchecked and unknown results and nothing for a message without a header or with one that cannot be read", async ({
  page,
}) => {
  const { card } = await openCard(page, rowFrom(MICROSOFT_SENDER));
  await openDetails(page, card);
  await expect(lineOf(page)).toHaveText(
    "Checked by your mail server: SPF passed, DKIM passed, DMARC passed.",
  );
  await openDetails(page, await openThreadOf(page, FAILED_SENDER));
  await expect(lineOf(page)).toHaveText(
    `Checked by ${AUTHSERV}: SPF passed, DKIM failed, DMARC failed.`,
  );
  await openDetails(page, await openThreadOf(page, UNCHECKED_SENDER));
  await expect(lineOf(page)).toHaveText(
    `Checked by ${AUTHSERV}: SPF not checked, DKIM unknown, DMARC not checked.`,
  );
  for (const sender of [REPLY_SENDER, UNREADABLE_SENDER]) {
    const details = await openDetails(page, await openThreadOf(page, sender));
    await expect(details.getByRole("tab")).toHaveText(TABS);
    await expect(lineOf(page)).toHaveCount(0);
  }
});

test("the palette carries the command without a key; it opens the details of the newest open card and the focus comes back where it was", async ({
  page,
}) => {
  const row = rowFrom(NEWSLETTER_SENDER);
  await openCard(page, row);
  await rowAt(page, row).focus();
  await page.keyboard.press(`${MOD}+k`);
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await expect(palette.getByRole("combobox")).toBeFocused();
  await page.keyboard.type("details");
  await expect(palette.getByRole("option")).toHaveText([DETAILS]);
  await page.keyboard.press("Enter");
  await expect(dialog(page)).toBeVisible();
  await expect(lineOf(page)).toContainText(`Checked by ${AUTHSERV}`);
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(rowAt(page, row)).toBeFocused();
});

test("the details read in Dutch and in the pseudo-locale, the host and the source in their own direction", async ({
  page,
}) => {
  const [phone] = VIEWPORTS;
  await page.setViewportSize({ width: phone.width, height: phone.height });
  const row = rowFrom(NEWSLETTER_SENDER);
  await mockSignedIn(page);
  await mockMail(page, READ_INBOX);
  await mockAccounts(page, ROWS);
  await mockPreferences(page, { locale: "nl" });
  await page.clock.setFixedTime(FIXED_NOW);
  await page.goto(pathOf(row));
  await cards(page).last().getByRole("button", { name: "Berichtdetails" }).click();
  const dutch = page.getByRole("dialog", { name: "Berichtdetails" });
  await expect(dutch.getByText(/Gecontroleerd door/)).toHaveText(
    `Gecontroleerd door ${AUTHSERV}: SPF geslaagd, DKIM geslaagd, DMARC geslaagd.`,
  );
  await expect(dutch.getByRole("tab")).toHaveText(["Weergave", "Platte tekst", "Bron"]);
  await expect(await showTab(dutch, "Platte tekst")).toHaveText(
    "Dit bericht heeft geen versie in platte tekst.",
  );
  await expect(dutch.getByRole("link", { name: "Het bericht downloaden" })).toBeVisible();
  await settled(page);
  await expect.soft(page).toHaveScreenshot("inspector-nl-light-phone.png");
  await page.keyboard.press("Escape");
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "en-XA");
  });
  await page.goto(pathOf(row));
  await cards(page)
    .last()
    .getByRole("button", { name: /Mésságé détáíls/ })
    .click();
  const pseudo = page.getByRole("dialog", { name: /Mésságé détáíls/ });
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
  // The host is a name the server wrote and keeps its own direction; so does the source.
  expect(await directionOf(pseudo.locator("bdi"))).toBe("ltr");
  await showTab(pseudo, "Sóúrçé");
  await expect(pseudo.locator("pre")).toContainText("From: sender@example.test");
  expect(await directionOf(pseudo.locator("pre"))).toBe("ltr");
  await settled(page);
  await expect.soft(page).toHaveScreenshot("inspector-en-XA-light-phone.png");
});

test("on a phone the details fill the screen, the tabs share the row and the download fills the foot", async ({
  page,
}) => {
  const [phone] = VIEWPORTS;
  await page.setViewportSize({ width: phone.width, height: phone.height });
  const { card } = await openCard(page, rowFrom(NEWSLETTER_SENDER));
  const details = await openDetails(page, card);
  const box = await details.boundingBox();
  expect(box?.width).toBe(phone.width);
  expect(box?.height).toBe(phone.height);
  const widths = await Promise.all(
    (await details.getByRole("tab").all()).map(
      async (tab) => (await tab.boundingBox())?.width ?? 0,
    ),
  );
  expect(Math.max(...widths) - Math.min(...widths)).toBeLessThanOrEqual(SHARE_TOLERANCE_PX);
  const download = await details.getByRole("link", { name: "Download the message" }).boundingBox();
  const panel = await details.getByRole("tabpanel").boundingBox();
  expect(download?.width).toBeGreaterThanOrEqual((panel?.width ?? 0) - SHARE_TOLERANCE_PX);
});

test("the details are axe-clean and match their screenshots on every tab in both themes at both widths", async ({
  page,
}) => {
  for (const viewport of VIEWPORTS) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    for (const theme of THEMES) {
      await page.emulateMedia({ colorScheme: theme });
      const label = `the details in ${theme} at ${viewport.name} width`;
      await test.step(label, async () => {
        const { card } = await openCard(page, rowFrom(NEWSLETTER_SENDER));
        const details = await openDetails(page, card);
        await framesLoaded(page);
        await settled(page);
        expect.soft((await axeOn(page).analyze()).violations, `axe on ${label}`).toEqual([]);
        await expect.soft(page).toHaveScreenshot(`inspector-${theme}-${viewport.name}.png`);
        if (theme !== "light") {
          return;
        }
        for (const tab of ["Plain text", "Source"]) {
          await expect(await showTab(details, tab)).toBeVisible();
          await settled(page);
          expect.soft((await axeOn(page).analyze()).violations, `axe on ${tab}`).toEqual([]);
          const name = tab === "Source" ? "source" : "plain";
          await expect.soft(page).toHaveScreenshot(`inspector-${name}-${viewport.name}.png`);
        }
        await page.keyboard.press("Escape");
      });
    }
  }
});
