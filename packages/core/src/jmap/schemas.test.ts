// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import * as fc from "fast-check";
import { expect, test } from "vitest";

import { email, mailbox } from "../cache/fake-jmap";
import {
  changesAnswerSchema,
  coreLimitsSchema,
  countSchema,
  emailHeaderSchema,
  emailStateSchema,
  idSchema,
  mailboxSchema,
  queryAnswerSchema,
  responseSchema,
  sessionObjectSchema,
} from "./schemas";

const SESSION = {
  capabilities: { "urn:ietf:params:jmap:core": { maxCallsInRequest: 16, maxObjectsInGet: 500 } },
  accounts: { u1: { isReadOnly: false } },
  primaryAccounts: { "urn:ietf:params:jmap:mail": "u1" },
  apiUrl: "/api/jmap/acc-1",
  downloadUrl: "/api/jmap/acc-1/download/{accountId}/{blobId}/{name}?type={type}",
  state: "s1",
};

const DOWNLOAD_VARIABLES = ["{accountId}", "{blobId}", "{name}", "{type}"];

// Numbers on both sides of every bound of a count or a limit, plus values
// of other types.
const numberish = fc.oneof(
  fc.integer({ min: -3, max: 3 }),
  fc.double(),
  fc.constantFrom(Number.MAX_SAFE_INTEGER, 2 ** 53, -(2 ** 53), 0.5, -0),
  fc.string(),
  fc.boolean(),
  fc.constant(null),
);

function isSafeWhole(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value);
}

test("a count is a safe whole number of zero or more and nothing else", () => {
  fc.assert(
    fc.property(numberish, (value) => {
      expect(countSchema.safeParse(value).success).toBe(isSafeWhole(value) && value >= 0);
    }),
  );
});

test("a server limit is a safe whole number above zero and nothing else", () => {
  fc.assert(
    fc.property(numberish, (value) => {
      const limits = { maxCallsInRequest: value, maxObjectsInGet: 1, maxObjectsInSet: 0 };
      expect(coreLimitsSchema.safeParse(limits).success).toBe(isSafeWhole(value) && value > 0);
    }),
  );
});

test("an id is any string but the empty one", () => {
  fc.assert(
    fc.property(fc.oneof(fc.string(), numberish), (value) => {
      expect(idSchema.safeParse(value).success).toBe(typeof value === "string" && value !== "");
    }),
  );
});

test("a download template passes only on this origin and with all four variables", () => {
  const start = fc.constantFrom("/", "//", "/\\", "", "https://mail.example.test/");
  fc.assert(
    fc.property(start, fc.shuffledSubarray(DOWNLOAD_VARIABLES), (head, variables) => {
      const downloadUrl = `${head}download/${variables.join("/")}`;
      const passes = head === "/" && variables.length === DOWNLOAD_VARIABLES.length;
      expect(sessionObjectSchema.safeParse({ ...SESSION, downloadUrl }).success).toBe(passes);
    }),
  );
});

test("a session object names an endpoint on this origin and nothing else", () => {
  expect(sessionObjectSchema.parse({ ...SESSION, extra: true }).apiUrl).toBe("/api/jmap/acc-1");
  const refused = ["https://mail.example.test/jmap", "//mail.example.test/jmap", "api/jmap"];
  const outcomes = refused.map((apiUrl) => sessionObjectSchema.safeParse({ ...SESSION, apiUrl }));
  expect(outcomes.map((outcome) => outcome.success)).toEqual([false, false, false]);
});

test("a download template stays on this origin and carries its four variables", () => {
  const refused = [
    "https://mail.example.test/download/{accountId}/{blobId}/{name}?type={type}",
    "//mail.example.test/download/{accountId}/{blobId}/{name}?type={type}",
    "/api/jmap/acc-1/download/{accountId}/{blobId}/{name}",
    "/api/jmap/acc-1/download/{blobId}/{name}?type={type}",
  ];
  const outcomes = refused.map((downloadUrl) =>
    sessionObjectSchema.safeParse({ ...SESSION, downloadUrl }),
  );
  expect(outcomes.map((outcome) => outcome.success)).toEqual([false, false, false, false]);
  const { downloadUrl, ...lean } = SESSION;
  expect(sessionObjectSchema.safeParse(lean).success).toBe(false);
  expect(sessionObjectSchema.parse(SESSION).downloadUrl).toBe(downloadUrl);
});

test("a session object says of every account whether it takes a write", () => {
  const { accounts, ...lean } = SESSION;
  expect(sessionObjectSchema.safeParse(lean).success).toBe(false);
  expect(sessionObjectSchema.safeParse({ ...lean, accounts: { u1: {} } }).success).toBe(false);
  expect(sessionObjectSchema.parse(SESSION).accounts).toEqual(accounts);
});

test("an email header parses with its null fields and refuses a wrong keyword or date", () => {
  const row = email("e1", { receivedAt: "2026-01-01T00:00:01Z" });
  expect(emailHeaderSchema.parse({ ...row, sentAt: "2026-09-01T09:30:00+02:00" }).sentAt).toBe(
    "2026-09-01T09:30:00+02:00",
  );
  expect(emailHeaderSchema.parse({ ...row, cc: null, subject: null }).subject).toBeNull();
  const refused = [
    { ...row, keywords: { $seen: false } },
    { ...row, receivedAt: "2026-01-01T02:00:01+02:00" },
    { ...row, mailboxIds: [] },
    { ...row, from: [{ email: "sanne@example.test" }] },
  ];
  for (const wrong of refused) {
    expect(emailHeaderSchema.safeParse(wrong).success).toBe(false);
  }
  expect(emailStateSchema.parse(row)).toEqual({
    id: "e1",
    threadId: "t-e1",
    mailboxIds: { inbox: true },
    keywords: {},
  });
});

test("a mailbox parses with and without the vendor property", () => {
  const { syncedEmails, ...plain } = mailbox("inbox", "inbox");
  expect(mailboxSchema.parse(plain).syncedEmails).toBeUndefined();
  expect(mailboxSchema.parse({ ...plain, syncedEmails }).syncedEmails).toBe(0);
  expect(mailboxSchema.safeParse({ ...plain, totalEmails: -1 }).success).toBe(false);
});

test("a Response object holds triples; the query and changes answers carry their optional fields", () => {
  const response = { methodResponses: [["Mailbox/get", { list: [] }, "a"]], sessionState: "s1" };
  expect(responseSchema.parse(response).methodResponses).toHaveLength(1);
  const odd = { methodResponses: [["Mailbox/get", { list: [] }]], sessionState: "s1" };
  expect(responseSchema.safeParse(odd).success).toBe(false);
  const query = { queryState: "2", position: 0, ids: ["e1"] };
  expect(queryAnswerSchema.parse(query).total).toBeUndefined();
  expect(queryAnswerSchema.parse({ ...query, total: 6, limit: 200 })).toMatchObject({
    total: 6,
    limit: 200,
  });
  const changes = {
    oldState: "1",
    newState: "2",
    hasMoreChanges: false,
    created: ["a"],
    updated: [],
    destroyed: [],
  };
  expect(changesAnswerSchema.parse(changes).created).toEqual(["a"]);
  expect(changesAnswerSchema.safeParse({ ...changes, created: [""] }).success).toBe(false);
});
