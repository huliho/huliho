// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page, Route } from "@playwright/test";

const ACCOUNTS_ROUTE = "**/api/accounts";
const DISCOVER_ROUTE = "**/api/accounts/discover";
const CREDENTIALS_ROUTE = "**/api/accounts/*/credentials";
// The server's default, so the page renders the sentence with it.
const PROBE_INTERVAL_MINUTES = 15;
const HOUR_MS = 3_600_000;

type Provider = "gmail" | "microsoft" | "fastmail" | "icloud" | "yahoo" | "generic";

export interface AccountRowBody {
  id: string;
  address: string;
  name: string;
  provider: Provider;
  kind: "jmap" | "imap";
  authMethod: "password" | "bearer" | "oauth2";
  stoppedCause: "credentials" | "connection" | null;
  stoppedAt: number | null;
  createdAt: number;
}

interface TargetBody {
  kind: "jmap" | "imap";
  [key: string]: unknown;
}

export interface FoundBody {
  status: "found";
  provider: Provider;
  kind: "jmap" | "imap";
  target: TargetBody;
  credentialKind: "password" | "appPassword" | "apiToken" | "oauth";
  host: string;
  oauthAvailable: boolean;
}

interface AccountsAnswer {
  status: number;
  error?: string;
  retryAfter?: number;
}

export interface AccountsAnswers {
  list?: number[];
  // A found server or a refusal per discovery; null and an empty queue read as not found.
  discover?: (FoundBody | AccountsAnswer | null)[];
  // Refusals in order; once they run out every add and reconnect passes.
  add?: AccountsAnswer[];
  credentials?: AccountsAnswer[];
}

interface CredentialBody {
  kind: string;
  password?: string;
  token?: string;
}

interface AddBody {
  address: string;
  provider: Provider;
  target: TargetBody;
  credential: CredentialBody;
}

interface Reconnect {
  id: string;
  credential: CredentialBody;
}

export interface Recorded {
  discoveries: string[];
  adds: AddBody[];
  credentials: Reconnect[];
}

const PROVIDER_NAMES: Record<Provider, string | null> = {
  gmail: "Gmail",
  microsoft: "Microsoft",
  fastmail: "Fastmail",
  icloud: "iCloud",
  yahoo: "Yahoo",
  generic: null,
};

export const FASTMAIL_FOUND: FoundBody = {
  status: "found",
  provider: "fastmail",
  kind: "jmap",
  target: { kind: "jmap", sessionUrl: "https://api.fastmail.com/jmap/session" },
  credentialKind: "apiToken",
  host: "api.fastmail.com",
  oauthAvailable: false,
};

export const GMAIL_FOUND: FoundBody = {
  status: "found",
  provider: "gmail",
  kind: "imap",
  target: {
    kind: "imap",
    username: "sanne@gmail.com",
    imap: { host: "imap.gmail.com", port: 993, tls: "implicit" },
    smtp: { host: "smtp.gmail.com", port: 465, tls: "implicit" },
  },
  credentialKind: "appPassword",
  host: "imap.gmail.com",
  oauthAvailable: false,
};

export const DOVECOT_FOUND: FoundBody = {
  status: "found",
  provider: "generic",
  kind: "imap",
  target: {
    kind: "imap",
    username: "sanne@dekker-mail.nl",
    imap: { host: "imap.dekker-mail.nl", port: 993, tls: "implicit" },
    smtp: { host: "smtp.dekker-mail.nl", port: 465, tls: "implicit" },
  },
  credentialKind: "password",
  host: "imap.dekker-mail.nl",
  oauthAvailable: false,
};

// One connected Fastmail row, as the list shows it.
export function accountRow(now: Date, overrides: Partial<AccountRowBody> = {}): AccountRowBody {
  return {
    id: "acc-1",
    address: "sanne@fastmail.com",
    name: "Fastmail",
    provider: "fastmail",
    kind: "jmap",
    authMethod: "bearer",
    stoppedCause: null,
    stoppedAt: null,
    createdAt: now.getTime() - HOUR_MS,
    ...overrides,
  };
}

function isAddBody(value: unknown): value is AddBody {
  return (
    typeof value === "object" && value !== null && typeof Reflect.get(value, "address") === "string"
  );
}

function isReconnectBody(value: unknown): value is Pick<Reconnect, "credential"> {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof Reflect.get(value, "credential") === "object"
  );
}

function addressOf(route: Route): string {
  const body: unknown = route.request().postDataJSON();
  const address: unknown =
    typeof body === "object" && body !== null ? Reflect.get(body, "address") : null;
  return typeof address === "string" ? address : "";
}

function refuse(route: Route, answer: AccountsAnswer): Promise<void> {
  return route.fulfill({
    status: answer.status,
    headers: answer.retryAfter === undefined ? {} : { "retry-after": String(answer.retryAfter) },
    json: { error: answer.error ?? "internal" },
  });
}

// The row the server would create: the provider's name or the mail domain.
function rowOf(body: AddBody, ordinal: number): AccountRowBody {
  return {
    id: `acc-${String(ordinal)}`,
    address: body.address,
    name: PROVIDER_NAMES[body.provider] ?? body.address.slice(body.address.indexOf("@") + 1),
    provider: body.provider,
    kind: body.target.kind,
    authMethod: body.credential.kind === "bearer" ? "bearer" : "password",
    stoppedCause: null,
    stoppedAt: null,
    createdAt: Date.now(),
  };
}

// Answers the list, the discovery, the add and the reconnect; every body
// sent is recorded and `answers` lets a test refuse a request before the
// next one passes.
export async function mockAccounts(
  page: Page,
  rows: AccountRowBody[],
  answers: AccountsAnswers = {},
): Promise<Recorded> {
  const recorded: Recorded = { discoveries: [], adds: [], credentials: [] };
  let listed = rows;
  await page.route(ACCOUNTS_ROUTE, (route) => {
    if (route.request().method() === "POST") {
      const body: unknown = route.request().postDataJSON();
      if (!isAddBody(body)) {
        throw new Error("the add carried no account");
      }
      recorded.adds.push(body);
      const answer = answers.add?.shift();
      if (answer !== undefined) {
        return refuse(route, answer);
      }
      const row = rowOf(body, listed.length + 1);
      listed = [...listed, row];
      return route.fulfill({ status: 201, json: row });
    }
    const status = answers.list?.shift();
    if (status !== undefined && status !== 200) {
      return refuse(route, { status });
    }
    return route.fulfill({
      json: { accounts: listed, probeIntervalMinutes: PROBE_INTERVAL_MINUTES },
    });
  });
  await page.route(DISCOVER_ROUTE, (route) => {
    recorded.discoveries.push(addressOf(route));
    const answer = answers.discover?.shift() ?? null;
    if (answer === null) {
      return route.fulfill({ json: { status: "notFound" } });
    }
    return typeof answer.status === "number"
      ? refuse(route, answer)
      : route.fulfill({ json: answer });
  });
  await page.route(CREDENTIALS_ROUTE, (route) => {
    const id = route.request().url().split("/").at(-2) ?? "";
    const body: unknown = route.request().postDataJSON();
    if (!isReconnectBody(body)) {
      throw new Error("the reconnect carried no credential");
    }
    recorded.credentials.push({ id, credential: body.credential });
    const answer = answers.credentials?.shift();
    if (answer !== undefined) {
      return refuse(route, answer);
    }
    const row = listed.find((candidate) => candidate.id === id);
    return row === undefined
      ? refuse(route, { status: 404, error: "not_found" })
      : route.fulfill({ json: { ...row, stoppedCause: null, stoppedAt: null } });
  });
  return recorded;
}
