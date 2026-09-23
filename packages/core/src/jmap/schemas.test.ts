// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { email, mailbox } from "../cache/fake-jmap";
import {
  changesAnswerSchema,
  emailHeaderSchema,
  emailStateSchema,
  mailboxSchema,
  queryAnswerSchema,
  responseSchema,
  sessionObjectSchema,
} from "./schemas";

const SESSION = {
  capabilities: { "urn:ietf:params:jmap:core": { maxCallsInRequest: 16, maxObjectsInGet: 500 } },
  primaryAccounts: { "urn:ietf:params:jmap:mail": "u1" },
  apiUrl: "/api/jmap/acc-1",
  state: "s1",
};

test("a session object names an endpoint on this origin and nothing else", () => {
  expect(sessionObjectSchema.parse({ ...SESSION, extra: true }).apiUrl).toBe("/api/jmap/acc-1");
  const refused = ["https://mail.example.test/jmap", "//mail.example.test/jmap", "api/jmap"];
  const outcomes = refused.map((apiUrl) => sessionObjectSchema.safeParse({ ...SESSION, apiUrl }));
  expect(outcomes.map((outcome) => outcome.success)).toEqual([false, false, false]);
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
