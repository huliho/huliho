// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page, Request } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { PREVIEW_URL } from "../playwright.config";
import { caseSubject, corpusOfCases } from "./mail-corpus";
import { MAILBOXES } from "./mail-mocks";
import type { MailboxBody } from "./mail-mocks";
import { VIEWPORTS } from "./sweep";
import { INBOX_PATH, cards, frameIn, openInbox, pane, rowAt } from "./thread-pane";
import { corpusCases } from "./xss-corpus";

const INBOX_ID = "mb-inbox";
// Each payload meets the window's layer alone, in the app's own frame,
// as if the server's layer had let it through whole.
const CASES = corpusCases();
// The corpus holds at least this many cases.
const CASES_FLOOR = 250;
// Each case takes a few keys and a frame; the run gets this long.
const CORPUS_TIMEOUT_MS = 600_000;
// The canary every mail case tries to set on the top window.
const CANARY = "__x";
// A case that names this scheme gets its links clicked.
const SCRIPT_SCHEME = "javascript:";
// A link the pipeline routed opens through this path; any other link
// of a message has no address to follow.
const BARE_LINK = 'a:not([href*="/open#"])';
// How the console names a base address the app's policy refused. The
// window's layer parses a case under that policy before it drops the tag.
const BASE_REFUSED = "Setting the document's base URI";

// The inbox at the size of the corpus, every message read.
function sized(row: MailboxBody): MailboxBody {
  return { ...row, totalEmails: CASES.length, totalThreads: CASES.length, unreadEmails: 0 };
}

// The inbox holding one message per case, every other mailbox as the fixtures have it.
const CASE_INBOX = MAILBOXES.map((row) => (row.id === INBOX_ID ? sized(row) : row));

// What a rendered case must never leave in the app's document or its
// frame: the canary on the top window, a script or a handler in the
// frame, a frame that navigated away from its own document.
async function problemsOf(page: Page): Promise<string[]> {
  const frame = frameIn(cards(page).last());
  await expect(frame).not.toHaveAttribute("data-loading");
  return frame.evaluate((element, canary) => {
    const document = element instanceof HTMLIFrameElement ? element.contentDocument : null;
    if (document === null) {
      return ["the frame has no document of this origin"];
    }
    const handlers = Array.from(document.querySelectorAll("*")).flatMap((node) =>
      node.getAttributeNames().filter((name) => name.startsWith("on")),
    );
    return [
      ...Array.from(document.querySelectorAll("script"), () => "a script element"),
      ...handlers,
      ...(canary in window ? ["the canary on the top window"] : []),
      ...(document.location.href === "about:srcdoc"
        ? []
        : [`navigated to ${document.location.href}`]),
    ];
  }, CANARY);
}

// Clicks every link in sight in the message that goes through no route,
// where it stands, whatever lies over it.
async function clickBareLinks(page: Page): Promise<void> {
  const card = cards(page).last();
  await expect(frameIn(card)).not.toHaveAttribute("data-loading");
  for (const link of await card.frameLocator("iframe").locator(BARE_LINK).all()) {
    if (await link.isVisible()) {
      await link.click({ force: true });
    }
  }
}

interface Errors {
  logged: string[];
  // Names the case now on screen.
  show: (title: string) => void;
}

// The errors the page logs from here on, each named by the case on
// screen. A script that ran or was refused leaves one; the app's own
// policy refusing the base address of a case is the one error expected.
function errorsFrom(page: Page): Errors {
  const logged: string[] = [];
  let shown = "";
  page.on("pageerror", (error) => logged.push(`${shown}: ${error.message}`));
  page.on("console", (message) => {
    if (message.type() === "error" && !message.text().startsWith(BASE_REFUSED)) {
      logged.push(`${shown}: ${message.text()}`);
    }
  });
  return {
    logged,
    show: (title) => {
      shown = title;
    },
  };
}

// Whether a message's frame sent the request; a worker's request has no frame.
function isFromMessage(page: Page, request: Request): boolean {
  try {
    return request.frame() !== page.mainFrame();
  } catch {
    return false;
  }
}

test("every corpus case renders in the app's frame with no canary fired, no error logged, no request from the message and none leaving the page", async ({
  page,
}) => {
  test.setTimeout(CORPUS_TIMEOUT_MS);
  expect(CASES.length).toBeGreaterThanOrEqual(CASES_FLOOR);
  await page.setViewportSize(VIEWPORTS[1]);
  // A request to another origin is refused and named; so is any request a message sends.
  const left: string[] = [];
  const fromMessages: string[] = [];
  await page.context().route("**/*", (route) => {
    const url = route.request().url();
    if (new URL(url).origin === PREVIEW_URL) {
      return route.fallback();
    }
    left.push(url);
    return route.abort();
  });
  page.on("request", (request) => {
    if (isFromMessage(page, request)) {
      fromMessages.push(request.url());
    }
  });
  const mail = await openInbox(page, "right", INBOX_PATH, {
    mailboxes: CASE_INBOX,
    mail: { corpus: corpusOfCases(INBOX_ID, CASES) },
    ownClock: true,
  });
  const errors = errorsFrom(page);
  const found: string[] = [];
  await rowAt(page, 1).focus();
  for (const [index, entry] of CASES.entries()) {
    if (index > 0) {
      await page.keyboard.press("ArrowDown");
    }
    errors.show(entry.title);
    await page.keyboard.press("Enter");
    await expect(pane(page).getByRole("heading", { level: 2 })).toHaveText(caseSubject(entry));
    if (entry.payload.toLowerCase().includes(SCRIPT_SCHEME)) {
      await clickBareLinks(page);
    }
    const problems = await problemsOf(page);
    found.push(...problems.map((problem) => `${entry.title}: ${problem}`));
    await page.keyboard.press("Escape");
    await expect(rowAt(page, index + 1)).toBeFocused();
  }
  expect(found).toEqual([]);
  expect(errors.logged).toEqual([]);
  expect(left).toEqual([]);
  // No case carries a part of its own, so a message has nothing to ask for.
  expect(fromMessages).toEqual([]);
  // Nothing asked the proxy for a remote image; the reader allowed no sender.
  expect(mail.blobs.remote).toEqual([]);
  expect(await page.evaluate((canary) => canary in window, CANARY)).toBe(false);
});
