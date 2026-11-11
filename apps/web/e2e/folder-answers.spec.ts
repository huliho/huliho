// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The mocked server's answers to Mailbox/set, asked from a page through
// the proxy's route as the worker asks them: creates, updates and
// destroys with the errors of RFC 8621 section 2.5, the emails a destroy
// takes, the scripted refusal and the mail limits of the session.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { isRecord } from "./mail-folder-rules";
import type { Row } from "./mail-folder-rules";
import { FOLDER_MAILBOXES, mockMail } from "./mail-mocks";
import type { MailOptions, MockedMail } from "./mail-mocks";
import { MAX_UPLOAD_BYTES } from "./mail-sends";

const INBOX = "mb-inbox";
const ARCHIVE = "mb-archive";
const FACTUREN = "mb-facturen";
const VERBOUWING = "mb-verbouwing";
const OFFERTES = "mb-offertes";
// A page of the app's origin that is not the app, so nothing but the
// test's own requests reaches the mocked routes.
const BLANK = "/folder-answers";
// The emails the corpus files under Facturen.
const FACTUREN_EMAILS = 40;

type Answer = [string, Row];

async function opened(page: Page, options: MailOptions = {}): Promise<MockedMail> {
  const mocked = await mockMail(page, FOLDER_MAILBOXES, options);
  await page.route(`**${BLANK}`, (route) =>
    route.fulfill({ contentType: "text/html", body: "<!doctype html><title>Blank</title>" }),
  );
  await page.goto(BLANK);
  return mocked;
}

function isAnswer(value: unknown): value is Answer {
  return Array.isArray(value) && typeof value[0] === "string" && isRecord(value[1]);
}

// The object under the key, or an empty one.
function at(record: unknown, key: string): Row {
  const value: unknown = isRecord(record) ? new Map(Object.entries(record)).get(key) : null;
  return isRecord(value) ? value : {};
}

// One call from the page through the route, as the worker sends it.
async function call(page: Page, name: string, args: Row): Promise<Answer> {
  const answer = await page.evaluate(
    async ([method, asked]): Promise<unknown> => {
      const response = await fetch("/api/jmap/acc-1", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          using: ["urn:ietf:params:jmap:mail"],
          methodCalls: [[method, { accountId: "u1", ...asked }, "c"]],
        }),
      });
      const body: unknown = await response.json();
      return typeof body === "object" && body !== null
        ? Reflect.get(body, "methodResponses")
        : null;
    },
    [name, args] as const,
  );
  const first: unknown = Array.isArray(answer) ? answer[0] : null;
  if (!isAnswer(first)) {
    throw new Error("no answer");
  }
  return [first[0], first[1]];
}

async function set(page: Page, args: Row): Promise<Row> {
  const [name, answer] = await call(page, "Mailbox/set", args);
  expect(name).toBe("Mailbox/set");
  return answer;
}

async function mailboxes(page: Page, ids: string[] | null): Promise<Row[]> {
  const [, answer] = await call(page, "Mailbox/get", { ids });
  const list = answer["list"];
  return Array.isArray(list) ? list.filter((row) => isRecord(row)) : [];
}

test("a create lands with a new id under a parent it names by creation id", async ({ page }) => {
  await opened(page);
  const answer = await set(page, {
    create: {
      c: { name: "2026", parentId: "#f" },
      f: { name: "Bonnen", parentId: null },
    },
  });
  expect(answer["notCreated"]).toBeNull();
  const folder = at(answer["created"], "f");
  const child = at(answer["created"], "c");
  expect(folder).toMatchObject({ role: null, sortOrder: 10, totalEmails: 0, isSubscribed: true });
  expect(folder).not.toHaveProperty("name");
  expect(child["parentId"]).toBe(folder["id"]);
  const [, changed] = await call(page, "Mailbox/changes", { sinceState: answer["oldState"] });
  expect(changed["created"]).toEqual([folder["id"], child["id"]]);
  const shown = await mailboxes(page, [String(child["id"])]);
  expect(shown.map((row) => row["name"])).toEqual(["2026"]);
  const [, got] = await call(page, "Mailbox/get", { ids: ["mb-none"] });
  expect(got["notFound"]).toEqual(["mb-none"]);
});

test("an update renames, moves, hides and passes a role on in one call each", async ({ page }) => {
  await opened(page);
  const renamed = await set(page, { update: { [FACTUREN]: { name: "Rekeningen" } } });
  expect(renamed["updated"]).toEqual({ [FACTUREN]: null });
  const moved = await set(page, { update: { [FACTUREN]: { parentId: VERBOUWING } } });
  expect(moved["notUpdated"]).toBeNull();
  const hidden = await set(page, { update: { [FACTUREN]: { isSubscribed: false } } });
  expect(hidden["notUpdated"]).toBeNull();
  const passed = await set(page, {
    update: { [ARCHIVE]: { role: null }, [FACTUREN]: { role: "archive", isSubscribed: true } },
  });
  expect(passed["updated"]).toEqual({ [ARCHIVE]: { sortOrder: 10 }, [FACTUREN]: { sortOrder: 3 } });
  const rows = await mailboxes(page, [ARCHIVE, FACTUREN]);
  expect(rows.map((row) => [row["id"], row["role"]])).toEqual([
    [ARCHIVE, null],
    [FACTUREN, "archive"],
  ]);
  expect(rows[1]).toMatchObject({ name: "Rekeningen", parentId: VERBOUWING, isSubscribed: true });
});

test("every create and update the rows refuse answers its error", async ({ page }) => {
  const mocked = await opened(page);
  const before = JSON.stringify(mocked.server.mailboxes);
  const answer = await set(page, {
    create: {
      twin: { name: "Facturen", parentId: null },
      lost: { name: "Weg", parentId: "mb-none" },
      edged: { name: " Weg", parentId: null },
      odd: { name: "Weg", parentId: null, color: "red" },
      inbox: { name: "Tweede", parentId: null, role: "inbox" },
    },
    update: {
      [INBOX]: { name: "Post" },
      [VERBOUWING]: { parentId: OFFERTES },
      [ARCHIVE]: { isSubscribed: false },
      [FACTUREN]: { role: "junk" },
      "mb-none": { name: "Weg" },
    },
  });
  expect(answer["notCreated"]).toEqual({
    twin: { type: "invalidProperties", properties: ["name"] },
    lost: { type: "invalidProperties", properties: ["parentId"] },
    edged: { type: "invalidProperties", properties: ["name"] },
    odd: { type: "invalidProperties", properties: ["color"] },
    inbox: { type: "invalidProperties", properties: ["role"] },
  });
  expect(answer["notUpdated"]).toEqual({
    [INBOX]: { type: "forbidden" },
    [VERBOUWING]: { type: "invalidProperties", properties: ["parentId"] },
    [ARCHIVE]: { type: "forbidden" },
    [FACTUREN]: { type: "invalidProperties", properties: ["role"] },
    "mb-none": { type: "notFound" },
  });
  expect(answer["newState"]).toBe(answer["oldState"]);
  expect(JSON.stringify(mocked.server.mailboxes)).toBe(before);
});

test("a destroy the rows refuse answers its error and keeps the mail", async ({ page }) => {
  const mocked = await opened(page);
  const answer = await set(page, { destroy: [ARCHIVE, VERBOUWING, FACTUREN, "mb-none"] });
  expect(answer["notDestroyed"]).toEqual({
    [ARCHIVE]: { type: "forbidden" },
    [VERBOUWING]: { type: "mailboxHasChild" },
    [FACTUREN]: { type: "mailboxHasEmail" },
    "mb-none": { type: "notFound" },
  });
  expect(answer["destroyed"]).toBeNull();
  expect(mocked.server.corpus.lists.get(FACTUREN)?.ids).toHaveLength(FACTUREN_EMAILS);
});

test("a destroy with onDestroyRemoveEmails takes the mailbox's emails along", async ({ page }) => {
  const mocked = await opened(page);
  const ids = mocked.server.corpus.lists.get(FACTUREN)?.ids ?? [];
  expect(ids).toHaveLength(FACTUREN_EMAILS);
  const answer = await set(page, { destroy: [FACTUREN], onDestroyRemoveEmails: true });
  expect(answer["destroyed"]).toEqual([FACTUREN]);
  const [, gone] = await call(page, "Email/get", { ids: ids.slice(0, 2) });
  expect(gone["notFound"]).toEqual(ids.slice(0, 2));
  const [, changed] = await call(page, "Email/changes", { sinceState: answer["oldState"] });
  expect(changed["destroyed"]).toHaveLength(FACTUREN_EMAILS);
  expect((await mailboxes(page, null)).some((row) => row["id"] === FACTUREN)).toBe(false);
});

test("the scripted refusal answers every object and changes nothing", async ({ page }) => {
  const mocked = await opened(page);
  mocked.server.folderRefusal = "serverFail";
  const answer = await set(page, {
    create: { k: { name: "Nieuw", parentId: null } },
    update: { [FACTUREN]: { name: "Rekeningen" } },
    destroy: [OFFERTES],
  });
  expect(answer["notCreated"]).toEqual({ k: { type: "serverFail" } });
  expect(answer["notUpdated"]).toEqual({ [FACTUREN]: { type: "serverFail" } });
  expect(answer["notDestroyed"]).toEqual({ [OFFERTES]: { type: "serverFail" } });
  expect(answer["newState"]).toBe(answer["oldState"]);
});

test("the session names the mail limits and the top level follows them", async ({ page }) => {
  await opened(page, { mayCreateTopLevelMailbox: false });
  const session: unknown = await page.evaluate(async (): Promise<unknown> => {
    const response = await fetch("/api/jmap/acc-1/session");
    const body: unknown = await response.json();
    return body;
  });
  const account = at(at(session, "accounts"), "u1");
  expect(at(account["accountCapabilities"], "urn:ietf:params:jmap:mail")).toEqual({
    maxSizeMailboxName: 255,
    mayCreateTopLevelMailbox: false,
    maxSizeAttachmentsPerEmail: MAX_UPLOAD_BYTES,
  });
  const answer = await set(page, {
    create: {
      top: { name: "Boven", parentId: null },
      below: { name: "Onder", parentId: VERBOUWING },
    },
  });
  expect(answer["notCreated"]).toEqual({ top: { type: "forbidden" } });
  expect(answer["created"]).toHaveProperty("below");
});
