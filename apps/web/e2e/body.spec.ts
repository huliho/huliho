// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Locator, Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { mockAccounts } from "./account-mocks";
import { releaseBodies } from "./mail-answers";
import {
  CUT_ONCE_SENDER,
  CUT_TWICE_SENDER,
  NEWSLETTER_SENDER,
  NO_TEXT_SENDER,
  REPLY_SENDER,
  SHOP_URL,
} from "./mail-bodies";
import { FIXED_NOW, corpusFor, seedThread } from "./mail-corpus";
import { MAILBOXES, mockMail } from "./mail-mocks";
import { network } from "./network";
import { mockPreferences } from "./preference-mocks";
import { mockSignedIn } from "./session-mocks";
import { THEMES, VIEWPORTS, axeOn } from "./sweep";
import {
  INBOX_PATH,
  READ_INBOX,
  ROWS,
  bodiesLanded,
  cards,
  directionOf,
  frameIn,
  newestOf,
  openInbox,
  pathOf,
  rowAt,
  rowFrom,
  settled,
  singleRowFrom,
} from "./thread-pane";

const INBOX_ID = "mb-inbox";
const LOADING = "Loading…";
const OFFLINE = "Offline: this message isn’t stored on this device.";
const CUT_AT_FIRST = "This message was cut short at 4 MB.";
const CUT_AT_LARGE = "This message was cut short at 12 MB.";
// The caps of the two asks, as the request names them.
const FIRST_CAP = 4 * 1024 * 1024;
const LARGE_CAP = 12 * 1024 * 1024;
// A thread of this many unread messages opens every card at once.
const UNREAD_THREAD = 30;
const IN_FLIGHT = 2;
// Long enough for a third fetch to have gone out if nothing held it back.
const QUIET_MS = 500;
// How many body requests the server answers with the limit problem first.
const LIMITED = 2;
// A thread of this many messages to load after the limit.
const LIMITED_THREAD = 3;
// The frame of the newsletter stands at least this tall.
const NEWSLETTER_MIN_PX = 300;

// The open card of the row's thread.
async function openCard(page: Page, rowIndex: number): Promise<Locator> {
  await openInbox(page, "right", pathOf(rowIndex));
  const card = cards(page).last();
  await expect(card.getByRole("button").first()).toHaveAttribute("aria-expanded", "true");
  return card;
}

test("an HTML message renders in a sandboxed frame titled by who wrote and what about, as tall as its content", async ({
  page,
}) => {
  const row = rowFrom(NEWSLETTER_SENDER);
  const card = await openCard(page, row);
  const frame = frameIn(card);
  const newest = newestOf(row);
  // The name assistive technology reads for the frame.
  await expect(frame).toHaveAccessibleName(
    `Message from ${newest?.from.name ?? ""} about ${newest?.subject ?? ""}`,
  );
  await expect(frame).toHaveAttribute(
    "sandbox",
    "allow-same-origin allow-popups allow-popups-to-escape-sandbox",
  );
  await expect(frame).not.toHaveAttribute("data-loading");
  await expect(card.getByRole("status", { name: LOADING })).toHaveCount(0);
  const heading = card.frameLocator("iframe").getByRole("heading", { level: 1 });
  await expect(heading).toHaveText("Week 35: rentes, chips en de bouw");
  const box = await frame.boundingBox();
  expect(box?.height ?? 0).toBeGreaterThan(NEWSLETTER_MIN_PX);
  // The frame is as tall as its document, so the pane scrolls as one surface.
  const inside = await frame.evaluate((element) => {
    const document = element instanceof HTMLIFrameElement ? element.contentDocument : null;
    return { document: document?.documentElement.offsetHeight ?? 0, frame: element.clientHeight };
  });
  expect(Math.abs(inside.document - inside.frame)).toBeLessThanOrEqual(1);
});

test("a plain message shows its quotes by depth and its addresses as links through the open route", async ({
  page,
}) => {
  const card = await openCard(page, rowFrom(REPLY_SENDER));
  await expect(frameIn(card)).toHaveCount(0);
  const deep = card.getByText(/Kunnen jullie een offerte maken/);
  await expect(deep).toBeVisible();
  expect(
    await deep.evaluate((element) => element.closest("[data-depth]")?.getAttribute("data-depth")),
  ).toBe("2");
  const link = card.getByRole("link", { name: "https://www.blom-installaties.example/offertes" });
  await expect(link).toHaveAttribute("href", /^\/open#/);
  await expect(link).toHaveAttribute("target", "_blank");
});

test("a message without text says so", async ({ page }) => {
  const card = await openCard(page, rowFrom(NO_TEXT_SENDER));
  await expect(card.getByText("This message has no text.")).toBeVisible();
});

test("a cut message asks for the whole at the larger cap and offers the download when that is cut too", async ({
  page,
}) => {
  const once = rowFrom(CUT_ONCE_SENDER);
  const mail = await openInbox(page, "right", pathOf(once));
  const card = cards(page).last();
  await expect(card.getByText(CUT_AT_FIRST)).toBeVisible();
  await expect(card.getByRole("link", { name: "Download the message" })).toHaveCount(0);
  await card.getByRole("button", { name: "Show the whole message" }).click();
  await expect(card.getByText(/cut short/)).toHaveCount(0);
  expect(mail.server.asked).toEqual([FIRST_CAP, LARGE_CAP]);
  const twice = rowFrom(CUT_TWICE_SENDER);
  await rowAt(page, twice).click();
  const cut = cards(page).last();
  await expect(cut.getByText(CUT_AT_FIRST)).toBeVisible();
  await cut.getByRole("button", { name: "Show the whole message" }).click();
  await expect(cut.getByText(CUT_AT_LARGE)).toBeVisible();
  await expect(cut.getByRole("button", { name: "Show the whole message" })).toHaveCount(0);
  // The link downloads the whole message under the email's own blob id.
  // A same-origin download never passes a mocked route in Chromium, so
  // the address and the attribute are what this run can read.
  const download = cut.getByRole("link", { name: "Download the message" });
  await expect(download).toHaveAttribute(
    "href",
    /\/download\/u1\/.*\/message\.eml\?type=message%2Frfc822$/,
  );
  await expect(download).toHaveAttribute("download", "");
});

test("a card opened unread sends one Email/set and the row, the count and the dot follow; a refusal rolls them back with the toast", async ({
  page,
}) => {
  const mail = await openInbox(page, "right", INBOX_PATH, { mailboxes: MAILBOXES });
  const tree = page.getByRole("tree", { name: "Mailboxes" });
  await expect(tree.getByRole("treeitem", { name: "Inbox, 23 unread" })).toBeVisible();
  await expect(rowAt(page, 1)).toHaveAccessibleName(/unread$/);
  await rowAt(page, 1).click();
  const newest = mail.server.corpus.lists.get(INBOX_ID)?.exemplars[0] ?? "";
  await expect.poll(() => mail.server.sets.length).toBe(1);
  expect(mail.server.sets[0]?.["update"]).toEqual(
    expect.objectContaining({ [newest]: { "keywords/$seen": true } }),
  );
  await expect(rowAt(page, 1)).not.toHaveAccessibleName(/unread$/);
  const marked = mail.server.corpus.emails.get(newest);
  expect(marked?.seen).toBe(true);
  const left = mail.server.mailboxes.find((row) => row["id"] === INBOX_ID)?.["unreadEmails"];
  await expect(tree.getByRole("treeitem", { name: `Inbox, ${String(left)} unread` })).toBeVisible();
  // The next row's newest message is refused: the rows go back and the tab says so.
  const second = mail.server.corpus.lists.get(INBOX_ID)?.exemplars[1] ?? "";
  mail.server.refused.set(second, "forbidden");
  await rowAt(page, 2).click();
  await expect(page.getByText("Couldn’t mark as read. Your mail is safe.")).toBeVisible();
  await expect(rowAt(page, 2)).toHaveAccessibleName(/unread$/);
  expect(mail.server.corpus.emails.get(second)?.seen).toBe(false);
});

test("a thread of thirty unread messages opens with two body fetches in flight and no error state", async ({
  page,
}) => {
  const corpus = corpusFor(READ_INBOX);
  const thread = seedThread(
    corpus,
    INBOX_ID,
    Array.from({ length: UNREAD_THREAD }, (_, index) => ({
      body: {
        html: `<p>Message ${String(index + 1)} of the thread.</p><img src="https://cdn.example.test/${String(index)}.png" alt="">`,
      },
    })),
  );
  const mail = await openInbox(page, "right", INBOX_PATH, { mail: { corpus } });
  // Every card stands open and every body waits; two requests are out.
  mail.server.holdBodies = true;
  await page.goto(`${INBOX_PATH}/${thread[0]?.threadId ?? ""}`);
  await expect(cards(page)).toHaveCount(UNREAD_THREAD);
  await expect.poll(() => mail.server.heldBodies.length).toBe(IN_FLIGHT);
  await page.waitForTimeout(QUIET_MS);
  expect(mail.server.heldBodies).toHaveLength(IN_FLIGHT);
  releaseBodies(mail.server);
  await expect(frameIn(cards(page))).toHaveCount(UNREAD_THREAD);
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByRole("status", { name: LOADING })).toHaveCount(0);
  // Every card carries a bar with a region no eye sees. The pane scrolls
  // them all; the page around it stays as tall as the screen.
  await expect(cards(page).locator("[data-state]")).toHaveCount(UNREAD_THREAD);
  expect(
    await page.evaluate(() => document.documentElement.scrollHeight - window.innerHeight),
  ).toBe(0);
});

test("a body request past the server's limit goes back on the queue and lands", async ({
  page,
}) => {
  const corpus = corpusFor(READ_INBOX);
  const thread = seedThread(
    corpus,
    INBOX_ID,
    Array.from({ length: LIMITED_THREAD }, (_, index) => ({
      body: { html: `<p>Message ${String(index + 1)} after the limit.</p>` },
    })),
  );
  const mail = await openInbox(page, "right", INBOX_PATH, { mail: { corpus } });
  mail.server.limitBodies = LIMITED;
  await page.goto(`${INBOX_PATH}/${thread[0]?.threadId ?? ""}`);
  await expect(frameIn(cards(page))).toHaveCount(LIMITED_THREAD);
  await expect(page.getByRole("alert")).toHaveCount(0);
  expect(mail.server.limitBodies).toBe(0);
});

test("Tab reaches a link inside the frame, Enter opens it through the route and Escape closes the thread", async ({
  page,
  context,
}) => {
  await context.route(/^https:\/\/shop\.example\.test\//, (route) =>
    route.fulfill({ contentType: "text/html", body: "<title>shop</title><p>the sale</p>" }),
  );
  const row = rowFrom(NEWSLETTER_SENDER);
  const card = await openCard(page, row);
  const link = card.frameLocator("iframe").getByRole("link", { name: "Lees verder" });
  await expect(link).toHaveAttribute("title", SHOP_URL);
  await card.getByRole("button", { name: "Always for this sender" }).focus();
  await page.keyboard.press("Tab");
  await expect(link).toBeFocused();
  const opening = context.waitForEvent("page");
  await page.keyboard.press("Enter");
  const tab = await opening;
  await tab.waitForURL(SHOP_URL);
  await expect(tab.getByText("the sale")).toBeVisible();
  await tab.close();
  await expect(link).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/\/mail\/acc-1\/mb-inbox$/);
  await expect(rowAt(page, row)).toBeFocused();
});

interface Shown {
  name: string;
  sender: string;
}

// The states a message body takes, each by the sender who writes it.
const SHOWN: Shown[] = [
  { name: "newsletter", sender: NEWSLETTER_SENDER },
  { name: "reply", sender: REPLY_SENDER },
  { name: "cut", sender: CUT_TWICE_SENDER },
  { name: "no-text", sender: NO_TEXT_SENDER },
];

test("every body state is axe-clean and matches its screenshots in both themes at both widths", async ({
  page,
}) => {
  for (const viewport of VIEWPORTS) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    for (const theme of THEMES) {
      await page.emulateMedia({ colorScheme: theme });
      for (const shown of SHOWN) {
        const label = `${shown.name} in ${theme} at ${viewport.name} width`;
        await test.step(label, async () => {
          await openCard(page, rowFrom(shown.sender));
          await bodiesLanded(page);
          await settled(page);
          const results = await axeOn(page).analyze();
          expect.soft(results.violations, `axe on ${label}`).toEqual([]);
          await expect
            .soft(page)
            .toHaveScreenshot(`body-${shown.name}-${theme}-${viewport.name}.png`);
        });
      }
    }
  }
});

test("a body on its way shows the still lines and one the server does not hold says so", async ({
  page,
}) => {
  await page.setViewportSize(VIEWPORTS[1]);
  const row = rowFrom(NEWSLETTER_SENDER);
  const mail = await openInbox(page, "right", INBOX_PATH);
  mail.server.holdBodies = true;
  await rowAt(page, row).click();
  const card = cards(page).last();
  await expect(card.getByRole("status", { name: LOADING })).toBeVisible();
  await settled(page);
  await expect.soft(page).toHaveScreenshot("body-loading-light-desktop.png");
  const newest = newestOf(row)?.id ?? "";
  mail.server.gone.add(newest);
  releaseBodies(mail.server);
  await expect(card.getByText("This message isn’t on the server anymore.")).toBeVisible();
  await expect.soft(page).toHaveScreenshot("body-gone-light-desktop.png");
});

// Refuses every body request while `failing()` holds, as a proxy whose
// upstream is down does.
async function refuseBodiesWhile(page: Page, failing: () => boolean): Promise<void> {
  await page
    .context()
    .route("**/api/jmap/acc-1", (route) =>
      failing() && route.request().postData()?.includes("fetchHTMLBodyValues") === true
        ? route.fulfill({ status: 502, json: { error: "upstream_failed" } })
        : route.fallback(),
    );
}

test("a body the server refuses offers Try again, which lands it", async ({ page }) => {
  await page.setViewportSize(VIEWPORTS[1]);
  await openInbox(page, "right", INBOX_PATH);
  let failing = true;
  await refuseBodiesWhile(page, () => failing);
  await rowAt(page, rowFrom(REPLY_SENDER)).click();
  const card = cards(page).last();
  const alert = card.getByRole("alert");
  await expect(alert).toContainText(
    "Couldn’t load this message. Your mail is safe; nothing was lost.",
  );
  await settled(page);
  await expect.soft(page).toHaveScreenshot("body-failed-light-desktop.png");
  failing = false;
  await alert.getByRole("button", { name: "Try again" }).click();
  await expect(card.getByText(/Dank Pieter/)).toBeVisible();
});

test("offline a body this device holds shows and one it never got says so; back online that one lands by itself", async ({
  page,
}) => {
  await page.setViewportSize(VIEWPORTS[1]);
  const held = rowFrom(NEWSLETTER_SENDER);
  // A thread of one, which the list brought whole: only its body is missing.
  const missing = singleRowFrom(REPLY_SENDER);
  const newsletter = cards(page).last().frameLocator("iframe").getByRole("heading", { level: 1 });
  const sentence = cards(page).last().getByText(OFFLINE);
  await openInbox(page, "right", INBOX_PATH);
  const connect = await network(page);
  await rowAt(page, held).click();
  await expect(newsletter).toHaveText("Week 35: rentes, chips en de bouw");
  await connect(false);
  await rowAt(page, missing).click();
  await expect(sentence).toBeVisible();
  await expect(cards(page).last().getByRole("button", { name: "Try again" })).toHaveCount(0);
  await settled(page);
  await expect.soft(page).toHaveScreenshot("body-offline-light-desktop.png");
  await rowAt(page, held).click();
  await expect(newsletter).toHaveText("Week 35: rentes, chips en de bouw");
  await rowAt(page, missing).click();
  await expect(sentence).toBeVisible();
  await connect(true);
  await expect(
    cards(page)
      .last()
      .getByText(/Dank Pieter/),
  ).toBeVisible();
});

test("the body reads in Dutch and in the pseudo-locale", async ({ page }) => {
  const [phone] = VIEWPORTS;
  await page.setViewportSize({ width: phone.width, height: phone.height });
  const row = rowFrom(NEWSLETTER_SENDER);
  await mockSignedIn(page);
  await mockMail(page, READ_INBOX);
  await mockAccounts(page, ROWS);
  await mockPreferences(page, { locale: "nl" });
  await page.clock.setFixedTime(FIXED_NOW);
  await page.goto(pathOf(row));
  const card = cards(page).last();
  await expect(
    card.getByText("Externe afbeeldingen van deze afzender zijn geblokkeerd."),
  ).toBeVisible();
  await expect(card.getByRole("button", { name: "Eén keer laden" })).toBeVisible();
  await expect(frameIn(card)).toHaveAttribute("title", /^Bericht van /);
  await bodiesLanded(page);
  await settled(page);
  await expect.soft(page).toHaveScreenshot("body-newsletter-nl-light-phone.png");
  await page.addInitScript(() => {
    window.localStorage.setItem("PARAGLIDE_LOCALE", "en-XA");
  });
  await page.goto(pathOf(row));
  const pseudo = cards(page).last();
  await expect(pseudo.getByRole("button", { name: /Lóád óñçé/ })).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
  // The message keeps its own direction inside the right-to-left page.
  const heading = pseudo.frameLocator("iframe").getByRole("heading", { level: 1 });
  expect(await directionOf(heading)).toBe("ltr");
  await bodiesLanded(page);
  await settled(page);
  await expect.soft(page).toHaveScreenshot("body-newsletter-en-XA-light-phone.png");
});
