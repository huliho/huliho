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

// The answer of a call against an account that holds nothing.
function empty([name, , id]: Invocation): Invocation {
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
  return [name, { accountId: UPSTREAM, state: STATE, list: [], notFound: [] }, id];
}

function accountIdOf(route: Route): string {
  const parts = route.request().url().split("/");
  return parts[parts.indexOf("jmap") + 1] ?? "";
}

// Answers the proxy's two routes for any account with an empty mailbox,
// so the cache worker behind a signed-in page has something to poll. A
// route never reaches a shared worker's requests, so the page runs the
// dedicated worker, whose requests the context's routes do answer.
export async function mockEmptyMail(page: Page): Promise<void> {
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
      json: { methodResponses: calls(route).map((call) => empty(call)), sessionState: "s1" },
    });
  });
}
