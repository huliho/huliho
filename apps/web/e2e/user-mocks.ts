// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page, Route } from "@playwright/test";

const USERS_ROUTE = "**/api/users";
const RESET_ROUTE = "**/api/users/*/password-reset";
const HOUR_MS = 3_600_000;
const DAY_MS = 24 * HOUR_MS;
// The server's window for a one-time password.
const ISSUE_TTL_MS = DAY_MS;

export const ISSUED_ONE_TIME = "k7fq-2mzp-x4rt";

export interface UserRowBody {
  id: string;
  name: string;
  login: string;
  role: "owner" | "admin" | "member";
  lastActiveAt: number | null;
}

export interface CreateBody {
  name: string;
  login: string;
  role: UserRowBody["role"];
}

interface UsersAnswer {
  status: number;
  error?: string;
}

export interface UsersAnswers {
  list?: number[];
  create?: UsersAnswer[];
  reset?: UsersAnswer[];
}

// Four users at fixed distances from `now`, by sign-in name as the server lists them.
export function userRows(now: Date): UserRowBody[] {
  return [
    {
      id: "user-2",
      name: "Jonas",
      login: "jonas@example.com",
      role: "member",
      lastActiveAt: now.getTime() - DAY_MS,
    },
    {
      id: "user-1",
      name: "Mira",
      login: "mira@example.com",
      role: "owner",
      lastActiveAt: now.getTime(),
    },
    { id: "user-4", name: "Noor", login: "noor@example.com", role: "member", lastActiveAt: null },
    {
      id: "user-3",
      name: "Tomas",
      login: "tomas@example.com",
      role: "admin",
      lastActiveAt: now.getTime() - 21 * DAY_MS,
    },
  ];
}

function isCreateBody(value: unknown): value is CreateBody {
  return (
    typeof value === "object" && value !== null && typeof Reflect.get(value, "login") === "string"
  );
}

function createBody(route: Route): CreateBody {
  const body: unknown = route.request().postDataJSON();
  if (!isCreateBody(body)) {
    throw new Error("the create carried no user");
  }
  return body;
}

function refuse(route: Route, answer: UsersAnswer): Promise<void> {
  return route.fulfill({ status: answer.status, json: { error: answer.error ?? "internal" } });
}

function issued(): { oneTimePassword: string; expiresAt: number } {
  return { oneTimePassword: ISSUED_ONE_TIME, expiresAt: Date.now() + ISSUE_TTL_MS };
}

// Answers the list, records every create and reset and issues one secret
// for each; `answers` lets a test refuse a request before the next one passes.
export async function mockUsers(
  page: Page,
  rows: UserRowBody[],
  answers: UsersAnswers = {},
): Promise<{ creates: CreateBody[]; resets: string[] }> {
  const creates: CreateBody[] = [];
  const resets: string[] = [];
  let listed = rows;
  await page.route(USERS_ROUTE, (route) => {
    if (route.request().method() === "POST") {
      const body = createBody(route);
      creates.push(body);
      const answer = answers.create?.shift();
      if (answer !== undefined && answer.status !== 201) {
        return refuse(route, answer);
      }
      const user: UserRowBody = {
        id: `user-${String(listed.length + 1)}`,
        ...body,
        lastActiveAt: null,
      };
      listed = [...listed, user];
      return route.fulfill({ status: 201, json: { user, ...issued() } });
    }
    const status = answers.list?.shift();
    if (status !== undefined && status !== 200) {
      return refuse(route, { status });
    }
    return route.fulfill({ json: listed });
  });
  await page.route(RESET_ROUTE, (route) => {
    resets.push(route.request().url().split("/").at(-2) ?? "");
    const answer = answers.reset?.shift();
    if (answer !== undefined && answer.status !== 200) {
      return refuse(route, answer);
    }
    return route.fulfill({ json: issued() });
  });
  return { creates, resets };
}
