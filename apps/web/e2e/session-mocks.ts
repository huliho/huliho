// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page } from "@playwright/test";

const SESSION_ROUTE = "**/api/session";
const SESSIONS_ROUTE = "**/api/sessions";
const SESSION_ROW_ROUTE = "**/api/sessions/*";
const HOUR_MS = 3_600_000;
const DAY_MS = 24 * HOUR_MS;

const SESSION_BODY = {
  user: { id: "user-1", login: "mira@example.com", role: "owner" },
  organization: { id: "org-1", name: "mira@example.com" },
};

const SIGNED_OUT_BODY = { error: "unauthenticated" };

export interface SessionRowBody {
  id: string;
  current: boolean;
  device: { browser: string | null; os: string | null; phone: boolean; installed: boolean };
  address: string | null;
  createdAt: number;
  lastSeenAt: number;
}

export async function mockSignedIn(page: Page): Promise<void> {
  await page.route(SESSION_ROUTE, (route) => {
    if (route.request().method() === "GET") {
      return route.fulfill({ json: SESSION_BODY });
    }
    return route.fulfill({ status: 204 });
  });
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
