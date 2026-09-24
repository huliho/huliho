// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { listPage } from "@huliho/core";
import type {
  AccountRow,
  EmailHeader,
  ListPage,
  MailCache,
  Mailbox,
  MemberState,
  ThreadRow,
  WindowPage,
} from "@huliho/core";

const HOUR_MS = 3_600_000;
const CREATED_AT = 1_778_750_400_000;

// Screenshots must not age, so the rows sit at fixed times before a fixed now.
export const FIXED_NOW = new Date(2026, 4, 14, 10, 0);

const READ_ONLY = {
  mayReadItems: true,
  mayAddItems: false,
  mayRemoveItems: false,
  maySetSeen: false,
  maySetKeywords: false,
  mayCreateChild: false,
  mayRename: false,
  mayDelete: false,
  maySubmit: false,
};

// id, name, role, sort order, total, unread and the parent, if any.
type Row = [string, string, string | null, number, number, number, string?];

const ROWS: Row[] = [
  ["mb-trash", "Trash", "trash", 5, 12, 0],
  ["mb-inbox", "Inbox", "inbox", 0, 1204, 23],
  ["mb-drafts", "Drafts", "drafts", 1, 2, 0],
  ["mb-sent", "Sent", "sent", 2, 310, 0],
  ["mb-archive", "Archive", "archive", 3, 4021, 0],
  ["mb-junk", "Junk", "junk", 4, 9, 1],
  ["mb-facturen", "Facturen", null, 10, 40, 3],
  ["mb-verbouwing", "Verbouwing", null, 10, 0, 0],
  ["mb-offertes", "Offertes", null, 10, 5, 0, "mb-verbouwing"],
];

function build(rows: readonly Row[]): Mailbox[] {
  return rows.map(([id, name, role, sortOrder, total, unread, parentId]) => ({
    id,
    name,
    parentId: parentId ?? null,
    role,
    sortOrder,
    totalEmails: total,
    unreadEmails: unread,
    totalThreads: total,
    unreadThreads: unread,
    myRights: READ_ONLY,
    isSubscribed: true,
  }));
}

function emptied([id, name, role, sortOrder, , , parentId]: Row): Row {
  return parentId === undefined
    ? [id, name, role, sortOrder, 0, 0]
    : [id, name, role, sortOrder, 0, 0, parentId];
}

// The six roles and three folders, one nested and one empty, listed out
// of order as a server may list them.
export const MAILBOXES: Mailbox[] = build(ROWS);

// The same tree with nothing in the inbox.
export const MAILBOXES_EMPTY_INBOX: Mailbox[] = build(
  ROWS.map((row) => (row[2] === "inbox" ? emptied(row) : row)),
);

export const INBOX_ID = "mb-inbox";

export const FASTMAIL: AccountRow = {
  id: "acc-1",
  address: "sanne@fastmail.com",
  name: "Fastmail",
  provider: "fastmail",
  kind: "jmap",
  authMethod: "bearer",
  stoppedCause: null,
  stoppedAt: null,
  createdAt: CREATED_AT,
};

export const GMAIL: AccountRow = {
  id: "acc-2",
  address: "s.bakker@gmail.com",
  name: "Gmail",
  provider: "gmail",
  kind: "imap",
  authMethod: "password",
  stoppedCause: null,
  stoppedAt: null,
  createdAt: CREATED_AT + HOUR_MS,
};

export const ACCOUNTS: AccountRow[] = [FASTMAIL, GMAIL];

// One row to draw: who wrote, what about, the preview, when and the
// flags, plus how many of the thread stand in the inbox.
export interface Draft {
  sender: string | null;
  subject: string | null;
  preview: string;
  at: Date;
  unread?: boolean;
  flagged?: boolean;
  attachment?: boolean;
  count?: number;
}

function today(hour: number, minute: number): Date {
  return new Date(2026, 4, 14, hour, minute);
}

function onDay(day: number, hour: number): Date {
  return new Date(2026, 4, day, hour, 0);
}

const DRAFTS: Draft[] = [
  {
    sender: "Mireille Dekker",
    subject: "Serverwissel zaterdagnacht, korte onderbreking",
    preview: "We verhuizen mail-03 tussen 01:00 en 02:30.",
    at: today(9, 41),
    unread: true,
  },
  {
    sender: "Jonas Verhulst",
    subject: "Q3 planning deck + herziene budgetsheet",
    preview: "Twee bijlagen: de deck is leidend.",
    at: today(9, 12),
    unread: true,
    attachment: true,
  },
  {
    sender: "Pieter Blom",
    subject: "Offerte badkamerrenovatie, herziene versie",
    preview: "Hierbij versie 3 met het tegelwerk erin.",
    at: today(8, 15),
    flagged: true,
    attachment: true,
    count: 14,
  },
  {
    sender: "De Koersbrief",
    subject: "Week 35: rentes, chips en de bouw",
    preview: "Deze week: de ECB houdt vast, chipexport knelt.",
    at: today(8, 2),
    unread: true,
  },
  {
    sender: "Anouk van der Meulen",
    subject: "Notes from Tuesday's retro",
    preview: "Long one, sorry. TL;DR at the top.",
    at: onDay(13, 16),
  },
  {
    sender: "Agenda · Kastanje Studio",
    subject: "Uitnodiging: Sprint review, vrijdag 10:00",
    preview: "Vrijdag 10:00 tot 10:45 · Studio 2.",
    at: onDay(12, 11),
  },
  {
    sender: "Tomas Lindqvist",
    subject: "Re: Coffee next week?",
    preview: "Works for me, Tuesday 14:00.",
    at: onDay(12, 9),
    count: 3,
  },
  {
    sender: "Femke Aalders",
    subject: "Factuur 2026-118, Kastanje Studio",
    preview: "In de bijlage de factuur voor augustus.",
    at: onDay(11, 14),
    attachment: true,
  },
  {
    sender: "Ruben Smit",
    subject: "Uren september",
    preview: "Mijn urenstaat staat in de gedeelde map.",
    at: onDay(11, 8),
  },
  {
    sender: null,
    subject: null,
    preview: "",
    at: onDay(7, 12),
  },
  {
    sender: "Marktplaats",
    subject: "Je advertentie verloopt bijna",
    preview: "Verleng binnen 3 dagen om zichtbaar te blijven.",
    at: new Date(2025, 9, 14, 8, 0),
  },
];

function header(id: string, draft: Draft, threadId: string): EmailHeader {
  return {
    id,
    blobId: id,
    threadId,
    mailboxIds: { [INBOX_ID]: true },
    keywords: draft.unread === true ? {} : { $seen: true },
    size: 2048,
    receivedAt: draft.at.toISOString(),
    messageId: [`${id}@example.test`],
    inReplyTo: null,
    references: null,
    sender: null,
    from: draft.sender === null ? null : [{ name: draft.sender, email: "someone@example.test" }],
    to: [{ name: "Sanne Bakker", email: "sanne@fastmail.com" }],
    cc: null,
    bcc: null,
    replyTo: null,
    subject: draft.subject,
    sentAt: null,
    hasAttachment: draft.attachment === true,
    preview: draft.preview,
  };
}

// The thread's other members sit in the inbox too, read; the flag lives
// on the exemplar.
function thread(exemplar: EmailHeader, draft: Draft): ThreadRow {
  const others = Array.from(
    { length: (draft.count ?? 1) - 1 },
    (_, index): [string, MemberState] => [
      `${exemplar.id}-m${String(index + 2)}`,
      { keywords: { $seen: true }, mailboxIds: { [INBOX_ID]: true } },
    ],
  );
  const own: MemberState = {
    keywords: { ...exemplar.keywords, ...(draft.flagged === true ? { $flagged: true } : {}) },
    mailboxIds: exemplar.mailboxIds,
  };
  const members = Object.fromEntries([...others, [exemplar.id, own]]);
  return { id: exemplar.threadId, emailIds: Object.keys(members), members };
}

// A page of rows from the drafts, newest first as the server serves
// them, read from the headers and the thread rows the way the worker
// reads them; `from` numbers the ids past an earlier page's.
export function pageOf(drafts: readonly Draft[] = DRAFTS, pending = 0, from = 0): ListPage {
  const exemplars = drafts.map((draft, index) => {
    const number = String(from + index + 1);
    return { draft, exemplar: header(`e-${number}`, draft, `t-${number}`) };
  });
  const held: WindowPage = {
    ids: exemplars.map(({ exemplar }) => exemplar.id),
    total: exemplars.length,
    pending,
    emails: Object.fromEntries(exemplars.map(({ exemplar }) => [exemplar.id, exemplar])),
    threads: Object.fromEntries(
      exemplars.map(({ draft, exemplar }) => [exemplar.threadId, thread(exemplar, draft)]),
    ),
  };
  return listPage(held, INBOX_ID);
}

export const INBOX_PAGE: ListPage = pageOf();

// How a fixture cache answers a page: with rows, with a refusal or not at all.
export type PageAnswer = ListPage | "never" | Error;

// A cache for stories and tests: pages by mailbox and number; the
// mailbox tree from the fixtures; every reveal a no-op it records.
export function fixtureCache(
  pages: Record<string, PageAnswer>,
): MailCache & { revealed: string[] } {
  const revealed: string[] = [];
  return {
    revealed,
    mailboxes: () => Promise.resolve(MAILBOXES),
    window: (_accountId, mailboxId, page) => {
      const answer = pages[`${mailboxId}/${String(page)}`];
      if (answer === undefined) {
        return Promise.resolve({ rows: [], total: 0, pending: 0 });
      }
      if (answer === "never") {
        return new Promise(() => undefined);
      }
      return answer instanceof Error ? Promise.reject(answer) : Promise.resolve(answer);
    },
    thread: () => Promise.resolve(null),
    reveal: (_accountId, mailboxId) => {
      revealed.push(mailboxId);
      return Promise.resolve();
    },
  };
}
