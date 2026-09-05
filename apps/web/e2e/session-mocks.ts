// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page, Route } from "@playwright/test";

const SESSION_ROUTE = "**/api/session";
const SESSIONS_ROUTE = "**/api/sessions";
const SESSION_ROW_ROUTE = "**/api/sessions/*";
const PASSWORD_ROUTE = "**/api/password";
const HOUR_MS = 3_600_000;
const DAY_MS = 24 * HOUR_MS;

const SESSION_BODY = {
  user: { id: "user-1", login: "mira@example.com", name: "Mira", role: "owner" },
  organization: { id: "org-1", name: "mira@example.com" },
  passwordChangeRequired: false,
};

const SIGNED_OUT_BODY = { error: "unauthenticated" };

export type MockRole = "owner" | "admin" | "member";

// Who the session is per role; each one is a row the users mocks list.
function actorOf(role: MockRole): { id: string; login: string; name: string } {
  if (role === "admin") {
    return { id: "user-3", login: "tomas@example.com", name: "Tomas" };
  }
  if (role === "member") {
    return { id: "user-2", login: "jonas@example.com", name: "Jonas" };
  }
  return { id: "user-1", login: "mira@example.com", name: "Mira" };
}

function sessionBody(role: MockRole): object {
  return { ...SESSION_BODY, user: { ...actorOf(role), role } };
}

export interface SessionRowBody {
  id: string;
  current: boolean;
  device: { browser: string | null; os: string | null; phone: boolean; installed: boolean };
  address: string | null;
  createdAt: number;
  lastSeenAt: number;
}

// A live session that answers until it signs out, as the server's would.
async function mockLiveSession(page: Page, body: () => object): Promise<void> {
  let signedIn = true;
  await page.route(SESSION_ROUTE, (route) => {
    if (route.request().method() === "DELETE") {
      signedIn = false;
      return route.fulfill({ status: 204 });
    }
    if (!signedIn) {
      return route.fulfill({ status: 401, json: SIGNED_OUT_BODY });
    }
    return route.fulfill({ json: body() });
  });
}

export async function mockSignedIn(page: Page, role: MockRole = "owner"): Promise<void> {
  await mockLiveSession(page, () => sessionBody(role));
}

export async function mockSignedOut(page: Page): Promise<void> {
  await page.route(SESSION_ROUTE, (route) => route.fulfill({ status: 401, json: SIGNED_OUT_BODY }));
}

// One page-scoped account: any credentials sign in, sign-out signs out.
export async function mockSessionFlow(page: Page): Promise<void> {
  let signedIn = false;
  await page.route(SESSION_ROUTE, (route) => {
    const method = route.request().method();
    if (method === "POST") {
      signedIn = true;
      return route.fulfill({ status: 204 });
    }
    if (method === "DELETE") {
      signedIn = false;
      return route.fulfill({ status: 204 });
    }
    if (signedIn) {
      return route.fulfill({ json: SESSION_BODY });
    }
    return route.fulfill({ status: 401, json: SIGNED_OUT_BODY });
  });
}

// Three sessions at fixed distances from `now`: this device, a phone and a laptop.
export function sessionRows(now: Date): SessionRowBody[] {
  return [
    {
      id: "s-current",
      current: true,
      device: { browser: "Firefox", os: "Linux", phone: false, installed: false },
      address: "203.0.113.7",
      createdAt: now.getTime() - DAY_MS,
      lastSeenAt: now.getTime(),
    },
    {
      id: "s-phone",
      current: false,
      device: { browser: "Chrome", os: "Android", phone: true, installed: true },
      address: "203.0.113.7",
      createdAt: now.getTime() - 3 * DAY_MS,
      lastSeenAt: now.getTime() - 2 * HOUR_MS,
    },
    {
      id: "s-mac",
      current: false,
      device: { browser: "Safari", os: "macOS", phone: false, installed: false },
      address: "198.51.100.23",
      createdAt: now.getTime() - 30 * DAY_MS,
      lastSeenAt: now.getTime() - 21 * DAY_MS,
    },
  ];
}

// Answers the list and records every DELETE: "*" for the collection, else the id.
// `listStatuses` lets a test fail one list request before the next one passes.
export async function mockSessions(
  page: Page,
  rows: SessionRowBody[],
  listStatuses: number[] = [],
): Promise<{ deletes: string[] }> {
  const deletes: string[] = [];
  await mockSignedIn(page);
  await page.route(SESSIONS_ROUTE, (route) => {
    if (route.request().method() === "DELETE") {
      deletes.push("*");
      return route.fulfill({ status: 204 });
    }
    const status = listStatuses.shift();
    if (status !== undefined && status !== 200) {
      return route.fulfill({ status, json: { error: "internal" } });
    }
    return route.fulfill({ json: rows });
  });
  await page.route(SESSION_ROW_ROUTE, (route) => {
    deletes.push(route.request().url().split("/").pop() ?? "");
    return route.fulfill({ status: 204 });
  });
  return { deletes };
}

export interface PasswordChangeBody {
  current?: string;
  new: string;
}

export interface PasswordAnswer {
  status: number;
  error?: string;
  retryAfter?: number;
}

function isChangeBody(value: unknown): value is PasswordChangeBody {
  return (
    typeof value === "object" && value !== null && typeof Reflect.get(value, "new") === "string"
  );
}

function recordedChange(route: Route): PasswordChangeBody {
  const body: unknown = route.request().postDataJSON();
  if (!isChangeBody(body)) {
    throw new Error("the password change carried no body");
  }
  return body;
}

// Answers the password change with `answers` in order and 204 once they
// run out; every body sent is recorded and `onChanged` sees each success.
export async function mockPasswordChange(
  page: Page,
  answers: PasswordAnswer[] = [],
  onChanged: () => void = () => undefined,
): Promise<{ changes: PasswordChangeBody[] }> {
  const changes: PasswordChangeBody[] = [];
  await page.route(PASSWORD_ROUTE, (route) => {
    changes.push(recordedChange(route));
    const answer = answers.shift() ?? { status: 204 };
    if (answer.status === 204) {
      onChanged();
      return route.fulfill({ status: 204 });
    }
    return route.fulfill({
      status: answer.status,
      headers: answer.retryAfter === undefined ? {} : { "retry-after": String(answer.retryAfter) },
      json: { error: answer.error ?? "internal" },
    });
  });
  return { changes };
}

// A session opened with a one-time password: forced until one change lands.
export async function mockForcedSession(page: Page): Promise<{ changes: PasswordChangeBody[] }> {
  let forced = true;
  await mockLiveSession(page, () => ({ ...SESSION_BODY, passwordChangeRequired: forced }));
  return mockPasswordChange(page, [], () => {
    forced = false;
  });
}
