// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Browser, Locator, Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { accountRow, mockAccounts } from "./account-mocks";
import {
  AUTHSERV,
  CUT_ONCE_SENDER,
  FAILED_SENDER,
  HERO_URL,
  MICROSOFT_SENDER,
  NEWSLETTER_HTML,
  NEWSLETTER_SENDER,
  failedBy,
  passedBy,
} from "./mail-bodies";
import { FIXED_NOW, caseBody, corpusFor, seedThread } from "./mail-corpus";
import { mockMail } from "./mail-mocks";
import type { PolicyStore } from "./policy-mocks";
import { mockPreferences } from "./preference-mocks";
import { mockSignedIn } from "./session-mocks";
import {
  INBOX_PATH,
  READ_INBOX,
  cards,
  frameIn,
  heightOf,
  openInbox,
  pathOf,
  rowFrom,
  screenOf,
} from "./thread-pane";
import { corpusCases } from "./xss-corpus";

const INBOX_ID = "mb-inbox";
const PROXY = "/api/remote-image?url=";
const BLOCKED = "Remote images are blocked for this sender.";
const LOADED_ONCE = "Images loaded for this message only.";
const FAILED_CHECK = "Images stay blocked: this message failed the server’s sender check.";
// The narrowest screen the layout holds and the height a touch target keeps.
const NARROW = { width: 320, height: 640 };
const HIT_TARGET_PX = 44;
// Another receiving server than the one a grant pinned.
const OTHER_AUTHSERV = "mx.other.example";
// The sender of the corpus case whose header the sender wrote.
const FORGED_SENDER = "sven@hosting.example";
// An address without a place to break, longer than a narrow bar is wide.
const LONG_SENDER = "communicatie.afdeling.nieuwsbrief@universiteit.example.edu";

function always(sender: string): string {
  return `Always loading images from ${sender}.`;
}

// The bar above the newest card.
function bar(page: Page): Locator {
  return cards(page).last().locator("[data-state]");
}

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

// The height of every button of the newest card.
async function buttonHeights(page: Page): Promise<number[]> {
  const buttons = await cards(page).last().getByRole("button").all();
  return Promise.all(buttons.map((button) => heightOf(button)));
}

// The images the frame loads, by their addresses.
async function imageSources(card: Locator): Promise<string[]> {
  return card
    .frameLocator("iframe")
    .locator("img")
    .evaluateAll((images) => images.map((image) => image.getAttribute("src") ?? ""));
}

test("remote images load only after Load once, through the server's proxy, for this view alone", async ({
  page,
}) => {
  const row = rowFrom(NEWSLETTER_SENDER);
  const mail = await openInbox(page, "right", pathOf(row));
  const card = cards(page).last();
  await expect(frameIn(card)).not.toHaveAttribute("data-loading");
  await expect(bar(page)).toContainText(BLOCKED);
  expect(mail.blobs.remote).toEqual([]);
  const blocked = await imageSources(card);
  expect(blocked.some((src) => src.startsWith("data:image/svg+xml"))).toBe(true);
  expect(blocked.some((src) => src.includes(PROXY))).toBe(false);
  // The sender's own logo loads from the download route.
  expect(blocked.some((src) => src.includes("/download/u1/") && src.includes("logo.png"))).toBe(
    true,
  );
  await bar(page).getByRole("button", { name: "Load once" }).focus();
  await page.keyboard.press("Enter");
  await expect(bar(page).getByRole("status")).toHaveText(LOADED_ONCE);
  await expect.poll(() => mail.blobs.remote).toEqual([HERO_URL]);
  expect((await imageSources(card)).some((src) => src.includes(PROXY))).toBe(true);
  // The button left with its press; the bar holds the focus it had.
  await expect(bar(page).getByRole("button")).toHaveCount(0);
  await expect(bar(page)).toBeFocused();
  // Another look at the thread starts blocked again.
  await page.goto(INBOX_PATH);
  await page.goto(pathOf(row));
  await expect(bar(page)).toContainText(BLOCKED);
});

// A second browser context on the same session, as another device.
async function otherDevice(browser: Browser, policies: PolicyStore, path: string): Promise<Page> {
  const context = await browser.newContext();
  const page = await context.newPage();
  await mockSignedIn(page);
  await mockMail(page, READ_INBOX, { policies });
  await mockAccounts(page, [accountRow(FIXED_NOW)]);
  await mockPreferences(page, { readingPane: "right" });
  await page.clock.setFixedTime(FIXED_NOW);
  await page.goto(path);
  return page;
}

test("Always for this sender writes the grant with the server's pin, holds on a second device and Stop takes it back", async ({
  page,
  browser,
}) => {
  const row = rowFrom(NEWSLETTER_SENDER);
  const mail = await openInbox(page, "right", pathOf(row));
  await expect(bar(page)).toContainText(BLOCKED);
  await bar(page).getByRole("button", { name: "Always for this sender" }).click();
  await expect(bar(page)).toContainText(always(NEWSLETTER_SENDER));
  expect(mail.policies.get(NEWSLETTER_SENDER)).toEqual({
    sender: NEWSLETTER_SENDER,
    key: "remoteContent",
    value: { allow: true, authserv: AUTHSERV },
  });
  await expect.poll(() => mail.blobs.remote).toEqual([HERO_URL]);
  const other = await otherDevice(browser, mail.policies, pathOf(row));
  await expect(bar(other)).toContainText(always(NEWSLETTER_SENDER));
  expect((await imageSources(cards(other).last())).some((src) => src.includes(PROXY))).toBe(true);
  await other.context().close();
  await bar(page).getByRole("button", { name: "Stop" }).click();
  await expect(bar(page)).toContainText(BLOCKED);
  expect(mail.policies.has(NEWSLETTER_SENDER)).toBe(false);
  expect((await imageSources(cards(page).last())).some((src) => src.includes(PROXY))).toBe(false);
});

// A thread from one sender whose messages carry different headers,
// oldest first: passed by the pinned server, failed, stamped elsewhere,
// without a header.
function pinnedThread() {
  const corpus = corpusFor(READ_INBOX);
  const from = { name: "De Koersbrief", email: NEWSLETTER_SENDER };
  const message = (header: string | null, words: string) => ({
    seen: true,
    from,
    body: {
      html: `<p>${words}</p>${NEWSLETTER_HTML}`,
      authenticationResults: header === null ? [] : [header],
    },
  });
  const messages = seedThread(corpus, INBOX_ID, [
    message(null, "Without a header."),
    message(` ${OTHER_AUTHSERV}; dmarc=pass header.from=koersbrief.example`, "Stamped elsewhere."),
    message(failedBy("koersbrief.example"), "Failed the check."),
    message(passedBy("koersbrief.example"), "Passed the check."),
  ]);
  return { corpus, threadId: messages[0]?.threadId ?? "" };
}

test("a grant loads a message that passes its pin and holds every other: a failed check, another server, no header", async ({
  page,
}) => {
  const { corpus, threadId } = pinnedThread();
  await openInbox(page, "right", `${INBOX_PATH}/${threadId}`, { mail: { corpus } });
  await page.getByRole("button", { name: /older message/ }).click();
  const open = cards(page);
  await expect(open).toHaveCount(4);
  // The newest passed; its grant pins the server that stamped it.
  await bar(page).getByRole("button", { name: "Always for this sender" }).click();
  await expect(bar(page)).toContainText(always(NEWSLETTER_SENDER));
  for (const index of [0, 1, 2]) {
    await open.nth(index).getByRole("button").first().click();
    const held = open.nth(index).locator("[data-state]");
    await expect(held).toContainText(FAILED_CHECK);
    await expect(held.getByRole("button", { name: "Load once" })).toBeVisible();
    await expect(held.getByRole("button", { name: "Always for this sender" })).toHaveCount(0);
  }
});

test("a forged header above a server that stamps none holds under a grant without a pin", async ({
  page,
}) => {
  // The corpus case that carries a header of the sender's own making.
  const forgery = corpusCases().find((entry) => entry.headers !== undefined);
  if (forgery === undefined) {
    throw new Error("the corpus holds no case with a header");
  }
  const corpus = corpusFor(READ_INBOX);
  const from = { name: "Sven Mulder", email: FORGED_SENDER };
  const messages = seedThread(corpus, INBOX_ID, [
    {
      seen: true,
      from,
      body: {
        html: '<p>Sent through a server that stamps nothing.</p><img src="https://status.hosting.example/a.png" alt="a">',
      },
    },
    { seen: true, from, body: caseBody(forgery) },
  ]);
  const mail = await openInbox(page, "right", `${INBOX_PATH}/${messages[0]?.threadId ?? ""}`, {
    mail: { corpus },
  });
  const open = cards(page);
  // The grant is given on the message without a header, so its pin is empty.
  await open.first().getByRole("button").first().click();
  const plain = open.first().locator("[data-state]");
  await expect(plain).toContainText(BLOCKED);
  await plain.getByRole("button", { name: "Always for this sender" }).click();
  await expect(plain).toContainText(always(FORGED_SENDER));
  expect(mail.policies.get(FORGED_SENDER)?.value).toEqual({ allow: true, authserv: null });
  const forged = open.last().locator("[data-state]");
  await expect(forged).toContainText(FAILED_CHECK);
  await expect(forged.getByRole("button", { name: "Always for this sender" })).toHaveCount(0);
});

test("a header of the Microsoft shape passes and a failed check offers Load once alone", async ({
  page,
}) => {
  const mail = await openInbox(page, "right", pathOf(rowFrom(MICROSOFT_SENDER)));
  await expect(bar(page)).toContainText(BLOCKED);
  await bar(page).getByRole("button", { name: "Always for this sender" }).click();
  await expect(bar(page)).toContainText(always(MICROSOFT_SENDER));
  expect(mail.policies.get(MICROSOFT_SENDER)?.value).toEqual({ allow: true, authserv: "" });
  await page.goto(pathOf(rowFrom(FAILED_SENDER)));
  await expect(bar(page)).toContainText(BLOCKED);
  await expect(bar(page).getByRole("button", { name: "Load once" })).toBeVisible();
  await expect(bar(page).getByRole("button", { name: "Always for this sender" })).toHaveCount(0);
});

test("a grant the server refuses is taken back and the card says so", async ({ page }) => {
  const row = rowFrom(NEWSLETTER_SENDER);
  const mail = await openInbox(page, "right", pathOf(row));
  await page
    .context()
    .route("**/api/sender-policies/**", (route) =>
      route.fulfill({ status: 500, json: { error: "internal" } }),
    );
  await bar(page).getByRole("button", { name: "Always for this sender" }).click();
  await expect(page.getByText("Couldn’t save that choice. Try again.")).toBeVisible();
  await expect(bar(page)).toContainText(BLOCKED);
  expect(mail.policies.size).toBe(0);
});

test.describe("at 320 on a touchscreen", () => {
  test.use({ viewport: NARROW, hasTouch: true });

  test("the card, the bar and the cut notice fit the screen and their buttons keep the touch height", async ({
    page,
  }) => {
    await openInbox(page, "right", pathOf(rowFrom(NEWSLETTER_SENDER)));
    await expect(bar(page).getByRole("button", { name: "Load once" })).toBeVisible();
    await expect(frameIn(cards(page).last())).not.toHaveAttribute("data-loading");
    expect(await overflowOf(page)).toBe(0);
    expect(Math.min(...(await buttonHeights(page)))).toBeGreaterThanOrEqual(HIT_TARGET_PX);
    await page.goto(pathOf(rowFrom(CUT_ONCE_SENDER)));
    const whole = cards(page).last().getByRole("button", { name: "Show the whole message" });
    await expect(whole).toBeVisible();
    expect(await overflowOf(page)).toBe(0);
    expect(Math.min(...(await buttonHeights(page)))).toBeGreaterThanOrEqual(HIT_TARGET_PX);
  });

  test("the standing sentence breaks a long sender address inside the bar", async ({ page }) => {
    const corpus = corpusFor(READ_INBOX);
    const messages = seedThread(corpus, INBOX_ID, [
      {
        seen: true,
        from: { name: "Universiteit", email: LONG_SENDER },
        body: { html: '<p>Nieuws.</p><img src="https://cdn.universiteit.example/a.png" alt="a">' },
      },
    ]);
    await openInbox(page, "right", `${INBOX_PATH}/${messages[0]?.threadId ?? ""}`, {
      mail: { corpus },
    });
    await bar(page).getByRole("button", { name: "Always for this sender" }).click();
    await expect(bar(page)).toContainText(always(LONG_SENDER));
    expect(await bar(page).evaluate((strip) => strip.scrollWidth - strip.clientWidth)).toBe(0);
    expect(await overflowOf(page)).toBe(0);
  });
});
