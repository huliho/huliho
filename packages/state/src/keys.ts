// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The one registry every query key comes from. A mail key starts with
// the account id, so one account's queries share a prefix.
export const queryKeys = {
  accounts: ["accounts"],
  consent: ["consent"],
  preferences: ["preferences"],
  senderPolicies: ["senderPolicies"],
  session: ["session"],
  sessions: ["sessions"],
  users: ["users"],
  mailboxes: (accountId: string) => [accountId, "mailboxes"] as const,
  // Every page of one list; a page adds its number.
  windows: (accountId: string, mailboxId: string) => [accountId, "window", mailboxId] as const,
  window: (accountId: string, mailboxId: string, page: number) =>
    [accountId, "window", mailboxId, page] as const,
  thread: (accountId: string, threadId: string) => [accountId, "thread", threadId] as const,
  body: (accountId: string, emailId: string) => [accountId, "body", emailId] as const,
  // The first part of one email's raw source, read for the inspector.
  source: (accountId: string, emailId: string) => [accountId, "source", emailId] as const,
} as const;

// The words a mail key carries in second place, so a cache message can
// name the queries it touches.
export const MAIL_KEY_WORDS: ReadonlySet<unknown> = new Set([
  "mailboxes",
  "window",
  "thread",
  "body",
  "source",
]);
