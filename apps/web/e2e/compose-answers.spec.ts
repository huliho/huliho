// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The mocked server's answers to a draft and a send, asked from a page
// through the proxy's routes as the worker asks them: a create with its
// tree, a reply in its thread, a replace, the creates the rows refuse,
// the upload route with its limit, the one identity, a send with the
// implicit Email/set after it, the scripted refusal, the hold and the
// session's submission capability.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";

import { isRecord } from "./mail-folder-rules";
import type { Row } from "./mail-folder-rules";
import { WRITABLE_MAILBOXES, mockMail } from "./mail-mocks";
import type { MockedMail } from "./mail-mocks";
import { MAX_CONCURRENT_UPLOADS, MAX_UPLOAD_BYTES, releaseSends } from "./mail-sends";

const DRAFTS = "mb-drafts";
const SENT = "mb-sent";
const INBOX = "mb-inbox";
const ACCOUNT = "u1";
// A page of the app's origin that is not the app, so nothing but the
// test's own requests reaches the mocked routes.
const BLANK = "/compose-answers";
const MAIL = "urn:ietf:params:jmap:mail";
const SUBMISSION = "urn:ietf:params:jmap:submission";
const USING = ["urn:ietf:params:jmap:core", MAIL, SUBMISSION];
// The drafts the mailbox holds before a test saves one.
const DRAFTS_BEFORE = 2;
// The file a test uploads and a limit it passes by one byte.
const FILE_TEXT = "Twee badkamers.";
const TIGHT_LIMIT = FILE_TEXT.length - 1;
const FILING = {
  [`mailboxIds/${DRAFTS}`]: null,
  [`mailboxIds/${SENT}`]: true,
  "keywords/$draft": null,
};

type Response = [string, Row, string];

async function opened(page: Page): Promise<MockedMail> {
  const mocked = await mockMail(page, WRITABLE_MAILBOXES);
  await page.route(`**${BLANK}`, (route) =>
    route.fulfill({ contentType: "text/html", body: "<!doctype html><title>Blank</title>" }),
  );
  await page.goto(BLANK);
  return mocked;
}

function isResponse(value: unknown): value is Response {
  return Array.isArray(value) && typeof value[0] === "string" && isRecord(value[1]);
}

// The responses of one request from the page, in order.
async function request(page: Page, calls: [string, Row, string][]): Promise<Response[]> {
  const answered = await page.evaluate(
    async ([using, methodCalls]): Promise<unknown> => {
      const response = await fetch("/api/jmap/acc-1", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ using, methodCalls }),
      });
      const body: unknown = await response.json();
      return typeof body === "object" && body !== null
        ? Reflect.get(body, "methodResponses")
        : null;
    },
    [USING, calls] as const,
  );
  return Array.isArray(answered) ? answered.filter((one) => isResponse(one)) : [];
}

// The arguments of the one response to a call on the account.
async function answer(page: Page, name: string, args: Row): Promise<Row> {
  const [first] = await request(page, [[name, { accountId: ACCOUNT, ...args }, "c"]]);
  expect(first?.[0]).toBe(name);
  return first?.[1] ?? {};
}

function at(record: unknown, key: string): Row {
  const value: unknown = isRecord(record) ? new Map(Object.entries(record)).get(key) : null;
  return isRecord(value) ? value : {};
}

function listOf(answered: Row): Row[] {
  const list = answered["list"];
  return Array.isArray(list) ? list.filter((row) => isRecord(row)) : [];
}

// A draft as the client sends it: the tree with its text, its HTML and
// the parts given.
function draft(subject: string, parts: Row[] = [], extra: Row = {}): Row {
  const alternative = {
    type: "multipart/alternative",
    subParts: [
      { type: "text/plain", partId: "t" },
      { type: "text/html", partId: "h" },
    ],
  };
  return {
    mailboxIds: { [DRAFTS]: true },
    keywords: { $draft: true, $seen: true },
    from: [{ name: "", email: "mira@example.com" }],
    to: [{ name: "Anna", email: "anna@example.test" }],
    subject,
    bodyStructure: { type: "multipart/mixed", subParts: [alternative, ...parts] },
    bodyValues: { t: { value: "Hallo Anna" }, h: { value: "<p>Hallo Anna</p>" } },
    ...extra,
  };
}

async function saved(page: Page, object: Row): Promise<Row> {
  const set = await answer(page, "Email/set", { create: { d: object } });
  expect(set["notCreated"]).toBeNull();
  return at(set["created"], "d");
}

async function upload(page: Page, text: string): Promise<{ status: number; body: unknown }> {
  return page.evaluate(async (body) => {
    const response = await fetch(`/api/jmap/acc-1/upload/u1`, {
      method: "POST",
      headers: { "content-type": "text/plain" },
      body,
    });
    const answered: unknown = await response.json();
    return { status: response.status, body: answered };
  }, text);
}

// A draft and its send in one request, filed into Sent on success.
function sending(subject: string): [string, Row, string][] {
  const submission = {
    accountId: ACCOUNT,
    create: { s: { identityId: "identity-1", emailId: "#d" } },
    onSuccessUpdateEmail: { "#s": FILING },
  };
  return [
    ["Email/set", { accountId: ACCOUNT, create: { d: draft(subject) } }, "c1"],
    ["EmailSubmission/set", submission, "c2"],
  ];
}

test("a draft lands in Drafts with its parts and reads back as it was given", async ({ page }) => {
  await opened(page);
  const uploaded = await upload(page, FILE_TEXT);
  expect(uploaded.status).toBe(200);
  const file: Row = isRecord(uploaded.body) ? uploaded.body : {};
  expect(file).toMatchObject({ accountId: ACCOUNT, type: "text/plain", size: FILE_TEXT.length });
  const attachment = { type: "text/plain", blobId: file["blobId"], name: "offerte.txt" };
  const parts = [{ ...attachment, disposition: "attachment" }];
  const set = await answer(page, "Email/set", { create: { d: draft("Offerte", parts) } });
  const created = at(set["created"], "d");
  expect(Object.keys(created).toSorted()).toEqual(["blobId", "id", "size", "threadId"]);
  const changed = await answer(page, "Email/changes", { sinceState: set["oldState"] });
  expect(changed["created"]).toEqual([created["id"]]);
  const [email] = listOf(
    await answer(page, "Email/get", {
      ids: [created["id"]],
      properties: ["subject", "keywords", "mailboxIds", "to", "attachments", "bodyValues"],
      fetchTextBodyValues: true,
    }),
  );
  expect(email).toMatchObject({
    subject: "Offerte",
    keywords: { $draft: true, $seen: true },
    mailboxIds: { [DRAFTS]: true },
    to: [{ name: "Anna", email: "anna@example.test" }],
    attachments: [{ name: "offerte.txt", blobId: file["blobId"], disposition: "attachment" }],
  });
  expect(JSON.stringify(email?.["bodyValues"])).toContain("Hallo Anna");
  const [drafts] = listOf(await answer(page, "Mailbox/get", { ids: [DRAFTS] }));
  expect(drafts?.["totalEmails"]).toBe(DRAFTS_BEFORE + 1);
});

test("a reply joins the thread of the message it answers", async ({ page }) => {
  const mocked = await opened(page);
  const parent = mocked.server.corpus.lists.get(INBOX)?.ids[0] ?? "";
  const threadId = mocked.server.corpus.emails.get(parent)?.threadId;
  const answers = { inReplyTo: [`${parent}@example.test`], references: [`${parent}@example.test`] };
  const created = await saved(page, draft("Re: Offerte", [], answers));
  expect(created["threadId"]).toBe(threadId);
});

test("a create beside a destroy replaces the draft with a new id", async ({ page }) => {
  await opened(page);
  const first = await saved(page, draft("Offerte"));
  const replaced = await answer(page, "Email/set", {
    create: { d: draft("Offerte, herzien") },
    destroy: [first["id"]],
  });
  expect(replaced["destroyed"]).toEqual([first["id"]]);
  const second = at(replaced["created"], "d");
  expect(second["id"]).not.toBe(first["id"]);
  const gone = await answer(page, "Email/get", { ids: [first["id"]], properties: ["id"] });
  expect(gone["notFound"]).toEqual([first["id"]]);
});

test("every create the rows refuse answers its error and files nothing", async ({ page }) => {
  const mocked = await opened(page);
  const set = await answer(page, "Email/set", {
    create: {
      twice: draft("Twee", [], { mailboxIds: { [DRAFTS]: true, [SENT]: true } }),
      lost: draft("Weg", [], { mailboxIds: { "mb-none": true } }),
      flat: { ...draft("Plat"), textBody: [{ partId: "t", type: "text/plain" }] },
      marked: draft("Rood", [], { keywords: { $draft: true, $label: true } }),
      unread: draft("Leeg", [], { bodyValues: {} }),
      missing: draft("Zoek", [{ type: "text/plain", blobId: "upload-404", name: "x.txt" }]),
    },
  });
  expect(set["notCreated"]).toEqual({
    twice: { type: "invalidProperties", properties: ["mailboxIds"] },
    lost: { type: "invalidProperties", properties: ["mailboxIds"] },
    flat: { type: "invalidProperties", properties: ["textBody"] },
    marked: { type: "invalidProperties", properties: ["keywords"] },
    unread: { type: "invalidProperties", properties: ["bodyValues"] },
    missing: { type: "blobNotFound", notFound: ["upload-404"] },
  });
  expect(set["created"]).toBeNull();
  expect(set["newState"]).toBe(set["oldState"]);
  expect(mocked.server.saved.size).toBe(0);
});

test("the upload route answers 413 past the scripted limit and keeps nothing", async ({ page }) => {
  const mocked = await opened(page);
  mocked.server.uploadLimit = TIGHT_LIMIT;
  const refused = await upload(page, FILE_TEXT);
  expect(refused).toEqual({ status: 413, body: { error: "too_large" } });
  expect(mocked.server.uploads.size).toBe(0);
  const taken = await upload(page, FILE_TEXT.slice(0, TIGHT_LIMIT));
  expect(taken.status).toBe(200);
});

test("the account answers one identity with its address and no name", async ({ page }) => {
  await opened(page);
  const every = await answer(page, "Identity/get", { ids: null });
  expect(listOf(every)).toEqual([
    expect.objectContaining({
      id: "identity-1",
      name: "",
      email: "mira@example.com",
      mayDelete: false,
    }),
  ]);
  const named = await answer(page, "Identity/get", { ids: ["identity-2"] });
  expect(named).toMatchObject({ list: [], notFound: ["identity-2"] });
});

test("a send goes out final and the implicit Email/set files the draft", async ({ page }) => {
  const mocked = await opened(page);
  const responses = await request(page, sending("Offerte"));
  expect(responses.map(([name, , id]) => [name, id])).toEqual([
    ["Email/set", "c1"],
    ["EmailSubmission/set", "c2"],
    ["Email/set", "c2"],
  ]);
  const created = at(responses[0]?.[1]["created"], "d");
  const draftId = created["id"];
  expect(at(responses[1]?.[1]["created"], "s")).toEqual({
    id: expect.any(String),
    threadId: created["threadId"],
    envelope: {
      mailFrom: { email: "mira@example.com", parameters: null },
      rcptTo: [{ email: "anna@example.test", parameters: null }],
    },
    sendAt: expect.any(String),
    undoStatus: "final",
    deliveryStatus: null,
    dsnBlobIds: [],
    mdnBlobIds: [],
  });
  expect(responses[2]?.[1]["updated"]).toEqual({ [String(draftId)]: null });
  const props = ["mailboxIds", "keywords"];
  const [filed] = listOf(await answer(page, "Email/get", { ids: [draftId], properties: props }));
  expect(filed).toMatchObject({ mailboxIds: { [SENT]: true }, keywords: { $seen: true } });
  expect(filed?.["keywords"]).not.toHaveProperty("$draft");
  expect(mocked.server.submissions).toEqual([
    expect.objectContaining({ identityId: "identity-1", emailId: draftId }),
  ]);
});

test("a send that names its envelope gets none back", async ({ page }) => {
  await opened(page);
  const emailId = (await saved(page, draft("Offerte")))["id"];
  const envelope = {
    mailFrom: { email: "mira@example.com" },
    rcptTo: [{ email: "anna@example.test" }],
  };
  const set = await answer(page, "EmailSubmission/set", {
    create: { s: { identityId: "identity-1", emailId, envelope } },
  });
  expect(at(set["created"], "s")).toMatchObject({ undoStatus: "final" });
  expect(at(set["created"], "s")).not.toHaveProperty("envelope");
});

test("the scripted refusal keeps the draft in Drafts", async ({ page }) => {
  const mocked = await opened(page);
  mocked.server.sendError = "forbiddenToSend";
  const responses = await request(page, sending("Offerte"));
  const draftId = at(responses[0]?.[1]["created"], "d")["id"];
  expect(responses[1]?.[1]["notCreated"]).toEqual({ s: { type: "forbiddenToSend" } });
  expect(responses[2]?.[0]).toBe("Email/set");
  expect(responses[2]?.[1]["updated"]).toBeNull();
  const kept = await answer(page, "Email/get", { ids: [draftId], properties: ["mailboxIds"] });
  expect(listOf(kept)[0]?.["mailboxIds"]).toEqual({ [DRAFTS]: true });
  expect(mocked.server.submissions).toEqual([]);
});

test("every submission the rows refuse answers its error and sends nothing", async ({ page }) => {
  const mocked = await opened(page);
  const nobody = await saved(page, draft("Niemand", [], { to: [] }));
  const set = await answer(page, "EmailSubmission/set", {
    create: {
      stranger: { identityId: "identity-2", emailId: nobody["id"] },
      lost: { identityId: "identity-1", emailId: "draft-404" },
      empty: { identityId: "identity-1", emailId: nobody["id"] },
    },
  });
  expect(set["notCreated"]).toEqual({
    stranger: { type: "invalidProperties", properties: ["identityId"] },
    lost: { type: "invalidProperties", properties: ["emailId"] },
    empty: { type: "noRecipients" },
  });
  expect(mocked.server.submissions).toEqual([]);
});

test("a held send waits until it is let through", async ({ page }) => {
  const mocked = await opened(page);
  mocked.server.holdSends = true;
  const pending = request(page, sending("Offerte"));
  await expect.poll(() => mocked.server.heldSends.length).toBe(1);
  expect(mocked.server.submissions).toEqual([]);
  releaseSends(mocked.server);
  const responses = await pending;
  expect(at(responses[1]?.[1]["created"], "s")).toMatchObject({ undoStatus: "final" });
});

test("the session carries the submission capability and the upload limits", async ({ page }) => {
  await opened(page);
  const session: unknown = await page.evaluate(async (): Promise<unknown> => {
    const response = await fetch("/api/jmap/acc-1/session");
    const body: unknown = await response.json();
    return body;
  });
  const capabilities = at(session, "capabilities");
  expect(at(capabilities, "urn:ietf:params:jmap:core")).toMatchObject({
    maxSizeUpload: MAX_UPLOAD_BYTES,
    maxConcurrentUpload: MAX_CONCURRENT_UPLOADS,
  });
  expect(capabilities).toHaveProperty([SUBMISSION]);
  expect(at(session, "primaryAccounts")).toHaveProperty([SUBMISSION], ACCOUNT);
  const account = at(at(session, "accounts"), ACCOUNT)["accountCapabilities"];
  expect(at(account, SUBMISSION)).toEqual({ maxDelayedSend: 0, submissionExtensions: {} });
  expect(at(account, MAIL)["maxSizeAttachmentsPerEmail"]).toBe(MAX_UPLOAD_BYTES);
});
