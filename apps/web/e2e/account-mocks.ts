// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Page, Route } from "@playwright/test";

const ACCOUNTS_ROUTE = "**/api/accounts";
const DISCOVER_ROUTE = "**/api/accounts/discover";
const CREDENTIALS_ROUTE = "**/api/accounts/*/credentials";
const ACCOUNT_ROW_ROUTE = "**/api/accounts/*";
const RETRY_ROUTE = "**/api/accounts/*/retry";
const CONSENT_START_ROUTE = "**/api/accounts/oauth/start";
const CONSENT_PENDING_ROUTE = "**/api/accounts/oauth/pending/*";
// Where a mocked start sends the window; the context answers it with a page.
const CONSENT_URL = "https://accounts.google.test/consent";
const CONSENT_STATE = "consent-1";
const CONSENT_PAGE = "<!doctype html><title>Provider</title><p>Consent page</p>";
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

interface StillStoppedAnswer {
  status: 409;
  cause: "credentials" | "connection";
}

// A 200 resumes the row; a 409 names the stop that stands; any other
// status is a refusal with its code.
type RetryAnswer = StillStoppedAnswer | AccountsAnswer;

export interface AccountsAnswers {
  list?: number[];
  // A found server or a refusal per discovery; null and an empty queue read as not found.
  discover?: (FoundBody | AccountsAnswer | null)[];
  // Refusals in order; once they run out every add and reconnect passes.
  add?: AccountsAnswer[];
  credentials?: AccountsAnswer[];
  // Answers per retry in order; once they run out every retry passes.
  retry?: RetryAnswer[];
  // Refusals per removal in order; once they run out every removal passes.
  remove?: AccountsAnswer[];
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
  retries: string[];
  deletes: string[];
}

interface ConsentBody {
  provider: Provider;
  address: string;
  accountId?: string;
}

interface OutcomeBody {
  status: "pending" | "done" | "denied";
  accountId?: string;
  cause?: string;
}

export interface ConsentAnswers {
  // Refusals for the start in order; once they run out every start answers the fixture URL.
  start?: AccountsAnswer[];
  // The poll answers in order, the last one repeating; a numeric status is a refusal.
  outcomes: (OutcomeBody | AccountsAnswer)[];
}

export interface RecordedConsent {
  starts: ConsentBody[];
  polls: number;
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

function isRefusal(answer: OutcomeBody | AccountsAnswer): answer is AccountsAnswer {
  return typeof answer.status === "number";
}

function isConsentBody(value: unknown): value is ConsentBody {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof Reflect.get(value, "provider") === "string"
  );
}

function isStillStopped(answer: RetryAnswer): answer is StillStoppedAnswer {
  return "cause" in answer;
}

function rowIdOf(route: Route, fromEnd: number): string {
  return route.request().url().split("/").at(fromEnd) ?? "";
}

// The list with one row changed; a row the server never had stays absent.
function patched(
  rows: AccountRowBody[],
  id: string,
  change: Partial<AccountRowBody>,
): AccountRowBody[] {
  return rows.map((row) => (row.id === id ? { ...row, ...change } : row));
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

// What the account routes share: the rows the server lists, what the
// page sent and the answers a test queued.
interface Mocked {
  listed: AccountRowBody[];
  recorded: Recorded;
  answers: AccountsAnswers;
}

type Handler = (route: Route) => Promise<void>;

// A pass clears the stop on the row and answers it; a row the server
// never had is not found.
function answerResumed(mocked: Mocked, route: Route, id: string): Promise<void> {
  mocked.listed = patched(mocked.listed, id, { stoppedCause: null, stoppedAt: null });
  const row = mocked.listed.find((candidate) => candidate.id === id);
  return row === undefined
    ? refuse(route, { status: 404, error: "not_found" })
    : route.fulfill({ json: row });
}

function removeRoute(mocked: Mocked): Handler {
  return (route) => {
    if (route.request().method() !== "DELETE") {
      return route.fallback();
    }
    const id = rowIdOf(route, -1);
    mocked.recorded.deletes.push(id);
    const answer = mocked.answers.remove?.shift();
    if (answer !== undefined) {
      return refuse(route, answer);
    }
    mocked.listed = mocked.listed.filter((row) => row.id !== id);
    return route.fulfill({ status: 204 });
  };
}

function listRoute(mocked: Mocked): Handler {
  return (route) => {
    if (route.request().method() === "POST") {
      const body: unknown = route.request().postDataJSON();
      if (!isAddBody(body)) {
        throw new Error("the add carried no account");
      }
      mocked.recorded.adds.push(body);
      const answer = mocked.answers.add?.shift();
      if (answer !== undefined) {
        return refuse(route, answer);
      }
      const row = rowOf(body, mocked.listed.length + 1);
      mocked.listed = [...mocked.listed, row];
      return route.fulfill({ status: 201, json: row });
    }
    const status = mocked.answers.list?.shift();
    if (status !== undefined && status !== 200) {
      return refuse(route, { status });
    }
    return route.fulfill({
      json: { accounts: mocked.listed, probeIntervalMinutes: PROBE_INTERVAL_MINUTES },
    });
  };
}

function discoverRoute(mocked: Mocked): Handler {
  return (route) => {
    mocked.recorded.discoveries.push(addressOf(route));
    const answer = mocked.answers.discover?.shift() ?? null;
    if (answer === null) {
      return route.fulfill({ json: { status: "notFound" } });
    }
    return typeof answer.status === "number"
      ? refuse(route, answer)
      : route.fulfill({ json: answer });
  };
}

function credentialsRoute(mocked: Mocked): Handler {
  return (route) => {
    const id = rowIdOf(route, -2);
    const body: unknown = route.request().postDataJSON();
    if (!isReconnectBody(body)) {
      throw new Error("the reconnect carried no credential");
    }
    mocked.recorded.credentials.push({ id, credential: body.credential });
    const answer = mocked.answers.credentials?.shift();
    return answer === undefined ? answerResumed(mocked, route, id) : refuse(route, answer);
  };
}

function retryRoute(mocked: Mocked): Handler {
  return (route) => {
    const id = rowIdOf(route, -2);
    mocked.recorded.retries.push(id);
    const answer = mocked.answers.retry?.shift() ?? { status: 200 };
    if (isStillStopped(answer)) {
      mocked.listed = patched(mocked.listed, id, { stoppedCause: answer.cause });
      return route.fulfill({ status: 409, json: { error: "still_stopped", cause: answer.cause } });
    }
    // A row the server does not have leaves the next list answer as well.
    if (answer.status === 404) {
      mocked.listed = mocked.listed.filter((row) => row.id !== id);
    }
    return answer.status === 200 ? answerResumed(mocked, route, id) : refuse(route, answer);
  };
}

// Answers the list, the discovery, the add, the reconnect, the retry and
// the removal; every body sent is recorded and `answers` lets a test
// refuse a request before the next one passes.
export async function mockAccounts(
  page: Page,
  rows: AccountRowBody[],
  answers: AccountsAnswers = {},
): Promise<Recorded> {
  const mocked: Mocked = {
    listed: rows,
    recorded: { discoveries: [], adds: [], credentials: [], retries: [], deletes: [] },
    answers,
  };
  // Registered first, so the routes below take precedence over it.
  await page.route(ACCOUNT_ROW_ROUTE, removeRoute(mocked));
  await page.route(ACCOUNTS_ROUTE, listRoute(mocked));
  await page.route(DISCOVER_ROUTE, discoverRoute(mocked));
  await page.route(CREDENTIALS_ROUTE, credentialsRoute(mocked));
  await page.route(RETRY_ROUTE, retryRoute(mocked));
  return mocked.recorded;
}

// Answers the start and the poll; the provider's page comes from the
// context, so the window has somewhere to go.
export async function mockConsent(page: Page, answers: ConsentAnswers): Promise<RecordedConsent> {
  const recorded: RecordedConsent = { starts: [], polls: 0 };
  const outcomes = [...answers.outcomes];
  await page
    .context()
    .route(`${CONSENT_URL}**`, (route) =>
      route.fulfill({ contentType: "text/html", body: CONSENT_PAGE }),
    );
  await page.route(CONSENT_START_ROUTE, (route) => {
    const body: unknown = route.request().postDataJSON();
    if (!isConsentBody(body)) {
      throw new Error("the start carried no consent");
    }
    recorded.starts.push(body);
    const refusal = answers.start?.shift();
    if (refusal !== undefined) {
      return refuse(route, refusal);
    }
    return route.fulfill({
      json: { url: `${CONSENT_URL}?state=${CONSENT_STATE}`, state: CONSENT_STATE },
    });
  });
  await page.route(CONSENT_PENDING_ROUTE, (route) => {
    recorded.polls += 1;
    const answer = outcomes.length > 1 ? outcomes.shift() : outcomes[0];
    if (answer === undefined) {
      return refuse(route, { status: 404, error: "not_found" });
    }
    return isRefusal(answer) ? refuse(route, answer) : route.fulfill({ json: answer });
  });
  return recorded;
}
