// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The mocked server's answers to a search, asked from a page through the
// proxy's route as the worker asks them: the conditions over the corpus,
// a search held until released or aborted and the scripted failure.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { releaseSearches } from "./mail-answers";
import { corpusFor, seedThread } from "./mail-corpus";
import type { Corpus } from "./mail-corpus";
import { MAILBOXES, mockMail } from "./mail-mocks";
import type { MockedMail } from "./mail-mocks";

const INBOX = "mb-inbox";
const ARCHIVE = "mb-archive";
const JUNK = "mb-junk";
const TRASH = "mb-trash";
const BLOM = { name: "Pieter Blom", email: "pieter@blom-installaties.example" };
// The event that aborts the search a page holds open.
const ABORT_EVENT = "abort-search";
// A page of the app's origin that is not the app, so nothing but the
// test's own requests reaches the mocked routes.
const BLANK = "/search-answers";

type Answer = [string, Record<string, unknown>];

// Four threads on four days, each in its own mailbox; the archive's
// holds a reply from another sender.
function corpus(): Corpus {
  const empty = MAILBOXES.map((row) => ({ ...row, totalEmails: 0, unreadEmails: 0 }));
  const built = corpusFor(empty);
  const factuur = [{ subject: "Factuur", from: BLOM }, { subject: "Re: Factuur" }];
  seedThread(built, INBOX, [{ subject: "Offerte badkamer", from: BLOM }], new Date("2026-05-13"));
  seedThread(built, JUNK, [{ subject: "Offerte goedkoop" }], new Date("2026-05-12"));
  seedThread(built, TRASH, [{ subject: "Oude offerte" }], new Date("2026-05-11"));
  seedThread(built, ARCHIVE, factuur, new Date("2026-05-10"));
  return built;
}

async function opened(page: Page): Promise<MockedMail> {
  const mocked = await mockMail(page, MAILBOXES, { corpus: corpus() });
  await page.route(`**${BLANK}`, (route) =>
    route.fulfill({ contentType: "text/html", body: "<!doctype html><title>Blank</title>" }),
  );
  await page.goto(BLANK);
  return mocked;
}

function isAnswer(value: unknown): value is Answer {
  return Array.isArray(value) && typeof value[0] === "string" && typeof value[1] === "object";
}

// One Email/query from the page through the route, newest first and one
// email per thread, as the worker sends a search.
async function query(page: Page, filter: unknown): Promise<Answer> {
  const answer = await page.evaluate(async (asked): Promise<unknown> => {
    const sort = [{ property: "receivedAt", isAscending: false }];
    const args = { accountId: "u1", filter: asked, sort, collapseThreads: true };
    const call = ["Email/query", args, "q"];
    const response = await fetch("/api/jmap/acc-1", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ using: ["urn:ietf:params:jmap:mail"], methodCalls: [call] }),
    });
    const body: unknown = await response.json();
    const responses: unknown =
      typeof body === "object" && body !== null ? Reflect.get(body, "methodResponses") : null;
    return responses;
  }, filter);
  const first: unknown = Array.isArray(answer) ? answer[0] : null;
  if (!isAnswer(first)) {
    throw new Error("no answer");
  }
  return [first[0], first[1]];
}

async function idsOf(page: Page, filter: unknown): Promise<unknown> {
  const [name, args] = await query(page, filter);
  expect(name).toBe("Email/query");
  return args["ids"];
}

test("a search answers its conditions over every mailbox the scope keeps", async ({ page }) => {
  await opened(page);
  const scope = { inMailboxOtherThan: [JUNK, TRASH] };
  expect(await idsOf(page, { ...scope, text: "OFFERTE" })).toEqual([`${INBOX}-x0`]);
  expect(await idsOf(page, { text: "offerte" })).toEqual([
    `${INBOX}-x0`,
    `${JUNK}-x0`,
    `${TRASH}-x0`,
  ]);
  expect(await idsOf(page, { ...scope, from: "pieter blom" })).toEqual([
    `${INBOX}-x0`,
    `${ARCHIVE}-x0`,
  ]);
  expect(await idsOf(page, { text: '"oude offerte"' })).toEqual([`${TRASH}-x0`]);
  // The junk message's own instant holds; the inbox message's does not.
  const range = { after: "2026-05-11T23:59:00Z", before: "2026-05-12T23:59:00Z" };
  expect(await idsOf(page, range)).toEqual([`${JUNK}-x0`]);
  expect(await idsOf(page, null)).toHaveLength(4);
  expect(await query(page, { subject: "x" })).toEqual(["error", { type: "unsupportedFilter" }]);
  expect(await query(page, { before: "2026-05-13" })).toEqual([
    "error",
    { type: "invalidArguments" },
  ]);
});

test("a held search waits for its release while a mailbox window answers", async ({ page }) => {
  const mocked = await opened(page);
  mocked.server.holdSearches = true;
  const search = idsOf(page, { text: "factuur" });
  await expect.poll(() => mocked.server.heldSearches.length).toBe(1);
  expect(await idsOf(page, { inMailbox: INBOX })).toEqual([`${INBOX}-x0`]);
  releaseSearches(mocked.server);
  expect(await search).toEqual([`${ARCHIVE}-x1`]);
});

test("a held search the page aborts ends there and the next one answers", async ({ page }) => {
  const mocked = await opened(page);
  mocked.server.holdSearches = true;
  const ended = page.evaluate(async (event) => {
    const controller = new AbortController();
    window.addEventListener(event, () => {
      controller.abort();
    });
    const call = ["Email/query", { accountId: "u1", filter: { text: "factuur" } }, "q"];
    try {
      await fetch("/api/jmap/acc-1", {
        method: "POST",
        body: JSON.stringify({ using: ["urn:ietf:params:jmap:mail"], methodCalls: [call] }),
        signal: controller.signal,
      });
      return "answered";
    } catch (error) {
      return error instanceof Error ? error.name : "failed";
    }
  }, ABORT_EVENT);
  await expect.poll(() => mocked.server.heldSearches.length).toBe(1);
  await page.evaluate((event) => window.dispatchEvent(new Event(event)), ABORT_EVENT);
  expect(await ended).toBe("AbortError");
  releaseSearches(mocked.server);
  expect(await idsOf(page, { text: "factuur" })).toEqual([`${ARCHIVE}-x1`]);
});

test("the scripted failure answers a search and leaves a mailbox window alone", async ({
  page,
}) => {
  const mocked = await opened(page);
  mocked.server.searchError = "serverUnavailable";
  expect(await query(page, { text: "factuur" })).toEqual(["error", { type: "serverUnavailable" }]);
  expect(await idsOf(page, { inMailbox: INBOX })).toEqual([`${INBOX}-x0`]);
});
