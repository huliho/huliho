// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page, Route } from "@playwright/test";

import { MAX_OBJECTS_IN_SET, jmapSessionBody } from "./api-bodies";
import { mockBlobs } from "./blob-mocks";
import type { Blobs } from "./blob-mocks";
import {
  answerCalls,
  asksBody,
  asksSet,
  methodCallsOf,
  noteArrival,
  serverFor,
} from "./mail-answers";
import type { Invocation, MailServer } from "./mail-answers";
import { arrive, corpusFor } from "./mail-corpus";
import type { Corpus, CorpusEmail } from "./mail-corpus";
import { asksSearch } from "./mail-search";
import { asksSend, mockUploads } from "./mail-sends";
import { mockPolicies } from "./policy-mocks";
import type { PolicyStore } from "./policy-mocks";

const SESSION_ROUTE = "**/api/jmap/*/session";
const API_ROUTE = "**/api/jmap/*";
// The problem a request past the server's concurrency gets (RFC 8620 section 3.6.1).
const LIMIT_PROBLEM = {
  type: "urn:ietf:params:jmap:error:limit",
  limit: "maxConcurrentRequests",
  status: 429,
};

export interface MailboxBody {
  id: string;
  name: string;
  parentId: string | null;
  role: string | null;
  sortOrder: number;
  totalEmails: number;
  unreadEmails: number;
  totalThreads: number;
  unreadThreads: number;
  myRights: Record<string, boolean>;
  isSubscribed: boolean;
  syncedEmails?: number;
}

interface Counts {
  id: string;
  name: string;
  role?: string;
  sortOrder: number;
  total: number;
  unread: number;
  parentId?: string;
}

// The reader may mark a message read and set its keywords; nothing else.
const RIGHTS = {
  mayReadItems: true,
  mayAddItems: false,
  mayRemoveItems: false,
  maySetSeen: true,
  maySetKeywords: true,
  mayCreateChild: false,
  mayRename: false,
  mayDelete: false,
  maySubmit: false,
};

function mailbox(counts: Counts): MailboxBody {
  return {
    id: counts.id,
    name: counts.name,
    parentId: counts.parentId ?? null,
    role: counts.role ?? null,
    sortOrder: counts.sortOrder,
    totalEmails: counts.total,
    unreadEmails: counts.unread,
    totalThreads: counts.total,
    unreadThreads: counts.unread,
    myRights: RIGHTS,
    isSubscribed: true,
  };
}

// The six roles and three folders, one of them nested and one empty;
// the roles arrive out of order, as a server may list them.
export const MAILBOXES: MailboxBody[] = [
  mailbox({ id: "mb-trash", name: "Trash", role: "trash", sortOrder: 5, total: 12, unread: 0 }),
  mailbox({ id: "mb-inbox", name: "Inbox", role: "inbox", sortOrder: 0, total: 1204, unread: 23 }),
  mailbox({ id: "mb-drafts", name: "Drafts", role: "drafts", sortOrder: 1, total: 2, unread: 0 }),
  mailbox({ id: "mb-sent", name: "Sent", role: "sent", sortOrder: 2, total: 310, unread: 0 }),
  mailbox({
    id: "mb-archive",
    name: "Archive",
    role: "archive",
    sortOrder: 3,
    total: 4021,
    unread: 0,
  }),
  mailbox({ id: "mb-junk", name: "Junk", role: "junk", sortOrder: 4, total: 9, unread: 1 }),
  mailbox({ id: "mb-facturen", name: "Facturen", sortOrder: 10, total: 40, unread: 3 }),
  mailbox({ id: "mb-verbouwing", name: "Verbouwing", sortOrder: 10, total: 0, unread: 0 }),
  mailbox({
    id: "mb-offertes",
    name: "Offertes",
    sortOrder: 10,
    total: 5,
    unread: 0,
    parentId: "mb-verbouwing",
  }),
];

// The same tree where every mailbox takes and gives up messages, as on
// a server that moves them.
export const WRITABLE_MAILBOXES: MailboxBody[] = MAILBOXES.map((row) => ({
  ...row,
  myRights: { ...row.myRights, mayAddItems: true, mayRemoveItems: true },
}));

// A mailbox on a server that lets the user manage the folders: INBOX is
// never renamed or deleted and a role mailbox never deleted (RFC 8621
// section 2).
function managed(row: MailboxBody): MailboxBody {
  const inbox = row.role === "inbox";
  const myRights = { ...row.myRights, mayCreateChild: true, mayRename: !inbox };
  return { ...row, myRights: { ...myRights, mayDelete: row.role === null } };
}

// The tree of WRITABLE_MAILBOXES with the folders managed.
export const FOLDER_MAILBOXES: MailboxBody[] = WRITABLE_MAILBOXES.map(managed);

function calls(route: Route): Invocation[] {
  return methodCallsOf(route.request().postDataJSON());
}

// The mailboxes as the server lists them; a native account's carry no
// synced count.
export function listedMailboxes(
  mailboxes: readonly MailboxBody[],
  vendor: boolean,
): Record<string, unknown>[] {
  return mailboxes.map((row) =>
    Object.fromEntries(Object.entries(row).filter(([key]) => vendor || key !== "syncedEmails")),
  );
}

function accountIdOf(route: Route): string {
  const parts = route.request().url().split("/");
  return parts[parts.indexOf("jmap") + 1] ?? "";
}

export interface MailOptions {
  // The messages behind the mailboxes; built from their counts without one.
  corpus?: Corpus;
  // Whether the account is a bridge account, whose mailboxes count their synced emails.
  vendor?: boolean;
  // The user's sender policies; a second context given the same store
  // sees the same grants.
  policies?: PolicyStore;
  // The account's quotas (RFC 9425 section 4.1); without them the
  // account lists no quota capability.
  quotas?: Record<string, unknown>[];
  // The most emails one write takes, so a folder job runs in rounds.
  maxObjectsInSet?: number;
  // Whether a mailbox may be created at the top level; true without it.
  mayCreateTopLevelMailbox?: boolean;
}

// The mocked mail behind a page: the server the routes answer from, a
// way to let new mail arrive in a mailbox, the policies on record and
// what the blob routes were asked.
export interface MockedMail {
  server: MailServer;
  arrive: (mailboxId: string) => CorpusEmail;
  policies: PolicyStore;
  blobs: Blobs;
}

// A body request, held while the server holds and answered with the
// limit problem while the server limits; a write, a send or a search
// held while the server holds those; any other request is answered at
// once.
async function answered(server: MailServer, route: Route): Promise<void> {
  const asked = calls(route);
  if (asksSet(asked) && server.holdSets) {
    await new Promise<void>((resolve) => {
      server.heldSets.push(resolve);
    });
  }
  if (asksSend(asked) && server.holdSends) {
    await new Promise<void>((resolve) => {
      server.heldSends.push(resolve);
    });
  }
  if (asksSearch(asked) && server.holdSearches) {
    await new Promise<void>((resolve) => {
      server.heldSearches.push(resolve);
    });
  }
  if (asksBody(asked)) {
    if (server.limitBodies > 0) {
      server.limitBodies -= 1;
      return route.fulfill({
        status: LIMIT_PROBLEM.status,
        contentType: "application/problem+json",
        json: LIMIT_PROBLEM,
      });
    }
    if (server.holdBodies) {
      await new Promise<void>((resolve) => {
        server.heldBodies.push(resolve);
      });
    }
  }
  return route.fulfill({
    json: { methodResponses: answerCalls(server, asked), sessionState: "s1" },
  });
}

// Answers the proxy's routes for any account with the mailboxes and the
// corpus, so the shell behind a signed-in page has a tree, a list and
// bodies to draw, with the sender policies, the blob routes and the
// upload route beside them. A route never reaches a shared worker's
// requests, so the page runs the dedicated worker, whose requests the
// context's routes do answer.
export async function mockMail(
  page: Page,
  mailboxes: MailboxBody[] = MAILBOXES,
  options: MailOptions = {},
): Promise<MockedMail> {
  const corpus = options.corpus ?? corpusFor(mailboxes);
  const listed = listedMailboxes(mailboxes, options.vendor === true);
  const server = serverFor(corpus, listed, options.quotas ?? null);
  server.topLevel = options.mayCreateTopLevelMailbox ?? true;
  const policies = options.policies ?? new Map();
  const context = page.context();
  await context.addInitScript(() => {
    Reflect.deleteProperty(window, "SharedWorker");
  });
  const shape = {
    vendor: options.vendor === true,
    quota: options.quotas !== undefined,
    maxObjectsInSet: options.maxObjectsInSet ?? MAX_OBJECTS_IN_SET,
    topLevel: server.topLevel,
  };
  await context.route(SESSION_ROUTE, (route) =>
    route.fulfill({ json: jmapSessionBody(accountIdOf(route), shape) }),
  );
  await context.route(API_ROUTE, (route) =>
    route.request().method() === "POST" ? answered(server, route) : route.fallback(),
  );
  await mockPolicies(context, policies);
  const blobs = await mockBlobs(context);
  await mockUploads(context, server);
  return {
    server,
    arrive: (mailboxId) => {
      const message = arrive(corpus, mailboxId);
      noteArrival(server, message.id, message.threadId);
      return message;
    },
    policies,
    blobs,
  };
}

// Refuses the session object while `failing()` holds, as a proxy whose
// upstream is down does; the mock behind answers once it lets go.
export async function refuseMailWhile(page: Page, failing: () => boolean): Promise<void> {
  await page
    .context()
    .route(SESSION_ROUTE, (route) =>
      failing()
        ? route.fulfill({ status: 502, json: { error: "upstream_unreachable" } })
        : route.fallback(),
    );
}

// Refuses every request that asks for a list while `failing()` holds,
// so the tree stands and the list alone fails; `holding()` keeps such a
// request open instead, the way a slow server does.
export async function refuseListWhile(
  page: Page,
  failing: () => boolean,
  holding: () => boolean = () => false,
): Promise<void> {
  await page.context().route(API_ROUTE, (route) => {
    const asksList = calls(route).some(([name]) => name === "Email/query");
    if (!asksList || route.request().method() !== "POST") {
      return route.fallback();
    }
    if (holding()) {
      return new Promise<void>(() => undefined);
    }
    return failing()
      ? route.fulfill({ status: 502, json: { error: "upstream_failed" } })
      : route.fallback();
  });
}
