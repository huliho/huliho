// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page, Route } from "@playwright/test";

const SESSION_ROUTE = "**/api/jmap/*/session";
const API_ROUTE = "**/api/jmap/*";
const CORE_CAPABILITY = "urn:ietf:params:jmap:core";
const MAIL_CAPABILITY = "urn:ietf:params:jmap:mail";
const UPSTREAM = "u1";
const STATE = "0";

type Invocation = [string, Record<string, unknown>, string];

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

const RIGHTS = {
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

function isInvocation(value: unknown): value is Invocation {
  return Array.isArray(value) && value.length === 3 && typeof value[0] === "string";
}

function calls(route: Route): Invocation[] {
  const body: unknown = route.request().postDataJSON();
  const listed: unknown =
    typeof body === "object" && body !== null ? Reflect.get(body, "methodCalls") : null;
  return Array.isArray(listed) ? listed.filter((call) => isInvocation(call)) : [];
}

// The session object of an account on the proxy, as the worker reads it.
function sessionBody(accountId: string): object {
  return {
    capabilities: {
      [CORE_CAPABILITY]: {
        maxSizeUpload: 0,
        maxConcurrentUpload: 0,
        maxSizeRequest: 1_048_576,
        maxConcurrentRequests: 4,
        maxCallsInRequest: 16,
        maxObjectsInGet: 500,
        maxObjectsInSet: 0,
        collationAlgorithms: ["i;unicode-casemap"],
      },
      [MAIL_CAPABILITY]: {},
    },
    accounts: {
      [UPSTREAM]: {
        name: "mira@example.com",
        isPersonal: true,
        isReadOnly: true,
        accountCapabilities: { [MAIL_CAPABILITY]: {} },
      },
    },
    primaryAccounts: { [MAIL_CAPABILITY]: UPSTREAM },
    username: "mira@example.com",
    apiUrl: `/api/jmap/${accountId}`,
    downloadUrl: `/api/jmap/${accountId}/download/{accountId}/{blobId}/{name}?type={type}`,
    uploadUrl: `/api/jmap/${accountId}/upload/{accountId}`,
    eventSourceUrl: `/api/jmap/${accountId}/events?types={types}&closeafter={closeafter}&ping={ping}`,
    state: "s1",
  };
}

// The answer of one call: the mailboxes for a Mailbox/get, nothing for
// every other read, since the account holds no mail.
function answer([name, , id]: Invocation, mailboxes: MailboxBody[]): Invocation {
  if (name.endsWith("/changes")) {
    return [
      name,
      {
        accountId: UPSTREAM,
        oldState: STATE,
        newState: STATE,
        hasMoreChanges: false,
        created: [],
        updated: [],
        destroyed: [],
      },
      id,
    ];
  }
  if (name === "Email/query") {
    return [
      name,
      { accountId: UPSTREAM, queryState: STATE, canCalculateChanges: false, position: 0, ids: [] },
      id,
    ];
  }
  const list = name === "Mailbox/get" ? mailboxes : [];
  return [name, { accountId: UPSTREAM, state: STATE, list, notFound: [] }, id];
}

function accountIdOf(route: Route): string {
  const parts = route.request().url().split("/");
  return parts[parts.indexOf("jmap") + 1] ?? "";
}

// Answers the proxy's two routes for any account with the mailboxes and
// no mail, so the shell behind a signed-in page has a tree to draw. A
// route never reaches a shared worker's requests, so the page runs the
// dedicated worker, whose requests the context's routes do answer.
export async function mockMail(page: Page, mailboxes: MailboxBody[] = MAILBOXES): Promise<void> {
  const context = page.context();
  await context.addInitScript(() => {
    Reflect.deleteProperty(window, "SharedWorker");
  });
  await context.route(SESSION_ROUTE, (route) =>
    route.fulfill({ json: sessionBody(accountIdOf(route)) }),
  );
  await context.route(API_ROUTE, (route) => {
    if (route.request().method() !== "POST") {
      return route.fallback();
    }
    return route.fulfill({
      json: {
        methodResponses: calls(route).map((call) => answer(call, mailboxes)),
        sessionState: "s1",
      },
    });
  });
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
