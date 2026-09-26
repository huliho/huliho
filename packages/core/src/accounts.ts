// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { retryAfterSeconds } from "./credentials";
import { CSRF_HEADERS } from "./http";
import { z } from "./schema";

const ACCOUNTS_ENDPOINT = "/api/accounts";
const DISCOVER_ENDPOINT = "/api/accounts/discover";
const CONSENT_START_ENDPOINT = "/api/accounts/oauth/start";
const CONSENT_PENDING_ENDPOINT = "/api/accounts/oauth/pending";

const providerSchema = z.enum(["gmail", "microsoft", "fastmail", "icloud", "yahoo", "generic"]);
const accountKindSchema = z.enum(["jmap", "imap"]);
const authMethodSchema = z.enum(["password", "bearer", "oauth2"]);
const stopCauseSchema = z.enum(["credentials", "connection"]);
const credentialKindSchema = z.enum(["password", "appPassword", "apiToken", "oauth"]);
const tlsModeSchema = z.enum(["implicit", "starttls"]);

const endpointSchema = z.object({
  host: z.string(),
  port: z.number().int(),
  tls: tlsModeSchema,
});

const accountTargetSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("jmap"), sessionUrl: z.string() }),
  z.object({
    kind: z.literal("imap"),
    username: z.string(),
    imap: endpointSchema,
    smtp: endpointSchema,
  }),
]);

const accountRowSchema = z.object({
  id: z.string(),
  address: z.string(),
  name: z.string(),
  provider: providerSchema,
  kind: accountKindSchema,
  authMethod: authMethodSchema,
  stoppedCause: stopCauseSchema.nullable(),
  stoppedAt: z.number().nullable(),
  createdAt: z.number(),
});

const accountListSchema = z.object({
  accounts: z.array(accountRowSchema),
  probeIntervalMinutes: z.number().int().positive(),
});

const foundServerSchema = z.object({
  provider: providerSchema,
  kind: accountKindSchema,
  target: accountTargetSchema,
  credentialKind: credentialKindSchema,
  host: z.string(),
  oauthAvailable: z.boolean(),
});

const discoverySchema = z.discriminatedUnion("status", [
  foundServerSchema.extend({ status: z.literal("found") }),
  z.object({ status: z.literal("notFound") }),
]);

const consentDeniedCauseSchema = z.enum([
  "accessDenied",
  "exchangeFailed",
  "noRefreshToken",
  "upstreamCredentials",
  "upstreamUnreachable",
  "upstreamInsecure",
  "upstreamUnsupported",
  "smtpAuthUnavailable",
  "failed",
]);

// The window goes wherever this says, so only an https URL passes.
const startedConsentSchema = z.object({
  url: z.url({ protocol: /^https$/ }),
  state: z.string().min(1),
});

const consentOutcomeSchema = z.discriminatedUnion("status", [
  z.object({ status: z.literal("pending") }),
  z.object({ status: z.literal("done"), accountId: z.string() }),
  z.object({ status: z.literal("denied"), cause: consentDeniedCauseSchema }),
]);

// The answer to a retry on a row that stays stopped.
const stillStoppedSchema = z.object({
  error: z.literal("still_stopped"),
  cause: stopCauseSchema,
});

const errorBodySchema = z.object({ error: z.string() });

export type Provider = z.infer<typeof providerSchema>;
export type AuthMethod = z.infer<typeof authMethodSchema>;
export type CredentialKind = z.infer<typeof credentialKindSchema>;
export type TlsMode = z.infer<typeof tlsModeSchema>;
export type AccountTarget = z.infer<typeof accountTargetSchema>;
export type AccountRow = z.infer<typeof accountRowSchema>;
export type AccountList = z.infer<typeof accountListSchema>;
export type FoundServer = z.infer<typeof foundServerSchema>;
export type ConsentDeniedCause = z.infer<typeof consentDeniedCauseSchema>;
export type StartedConsent = z.infer<typeof startedConsentSchema>;
// What the poll answers; gone once the consent ran out or was never this user's.
export type ConsentOutcome = z.infer<typeof consentOutcomeSchema> | { status: "gone" };
export type StopCause = z.infer<typeof stopCauseSchema>;
// What a retry did: the row runs again or the stop stands with its cause.
export type RetryResult =
  { status: "resumed"; account: AccountRow } | { status: "stillStopped"; cause: StopCause };

export interface RemoveOptions {
  // A removal that fires while the page unloads needs keepalive to finish.
  keepalive?: boolean;
}

type Found = Extract<z.infer<typeof discoverySchema>, { status: "found" }>;

export interface ConsentInput {
  provider: Provider;
  address: string;
  // The row whose tokens the consent replaces; absent for a new account.
  accountId?: string;
}

// What the user hands over, sent once at Connect; the tokens of a
// consent come from the provider, never from here.
export type Credential = { kind: "password"; password: string } | { kind: "bearer"; token: string };

export interface NewAccountInput {
  address: string;
  provider: Provider;
  target: AccountTarget;
  credential: Credential;
}

export type AccountsFailureCode =
  | "invalid_request"
  | "upstream_credentials"
  | "upstream_unreachable"
  | "upstream_insecure"
  | "upstream_unsupported"
  | "smtp_auth_unavailable"
  | "rate_limited"
  | "provider_not_configured"
  | "not_found"
  | "unauthenticated"
  | "unavailable";

const NAMED_FAILURES: readonly AccountsFailureCode[] = [
  "invalid_request",
  "upstream_credentials",
  "upstream_unreachable",
  "upstream_insecure",
  "upstream_unsupported",
  "smtp_auth_unavailable",
  "provider_not_configured",
  "not_found",
  "unauthenticated",
];

export class AccountsError extends Error {
  readonly code: AccountsFailureCode;
  // Seconds until the limiter lets the next attempt through; zero unless rate limited.
  readonly retryAfterSeconds: number;

  constructor(code: AccountsFailureCode, retryAfter = 0) {
    super(`accounts request failed: ${code}`);
    this.name = "AccountsError";
    this.code = code;
    this.retryAfterSeconds = retryAfter;
  }
}

export async function fetchAccounts(): Promise<AccountList> {
  const response = await fetch(ACCOUNTS_ENDPOINT);
  if (!response.ok) {
    throw new Error(`the accounts request failed with status ${String(response.status)}`);
  }
  return accountListSchema.parse(await response.json());
}

// The server behind the address; null when the chain found none.
export async function discoverServer(address: string): Promise<FoundServer | null> {
  const response = await send("POST", DISCOVER_ENDPOINT, { address });
  const discovery = discoverySchema.parse(await response.json());
  return discovery.status === "found" ? foundOf(discovery) : null;
}

export async function addAccount(input: NewAccountInput): Promise<AccountRow> {
  const response = await send("POST", ACCOUNTS_ENDPOINT, input);
  return accountRowSchema.parse(await response.json());
}

export async function replaceCredential(id: string, credential: Credential): Promise<AccountRow> {
  const url = `${ACCOUNTS_ENDPOINT}/${encodeURIComponent(id)}/credentials`;
  const response = await send("PUT", url, { credential });
  return accountRowSchema.parse(await response.json());
}

// Runs the check now with the credential the row holds; nothing travels
// but the id.
export async function retryAccount(id: string): Promise<RetryResult> {
  const response = await reach(`${ACCOUNTS_ENDPOINT}/${encodeURIComponent(id)}/retry`, {
    method: "POST",
    headers: CSRF_HEADERS,
  });
  if (response.status === 409) {
    const stop = stillStoppedSchema.safeParse(await response.json().catch(() => null));
    if (!stop.success) {
      throw new AccountsError("unavailable");
    }
    return { status: "stillStopped", cause: stop.data.cause };
  }
  if (!response.ok) {
    throw await failureOf(response);
  }
  return { status: "resumed", account: accountRowSchema.parse(await response.json()) };
}

// A row that is already gone is the outcome asked for, so 404 passes.
export async function removeAccount(id: string, options: RemoveOptions = {}): Promise<void> {
  const response = await reach(`${ACCOUNTS_ENDPOINT}/${encodeURIComponent(id)}`, {
    method: "DELETE",
    headers: CSRF_HEADERS,
    keepalive: options.keepalive === true,
  });
  if (!response.ok && response.status !== 404) {
    throw await failureOf(response);
  }
}

export async function startConsent(input: ConsentInput): Promise<StartedConsent> {
  const response = await send("POST", CONSENT_START_ENDPOINT, input);
  return startedConsentSchema.parse(await response.json());
}

// Cancel on the card: ends the caller's own open consent, so a window
// still at the provider lands nothing. One already gone is the outcome
// asked for, so 404 passes.
export async function endConsent(id: string): Promise<void> {
  const response = await reach(`${CONSENT_PENDING_ENDPOINT}/${encodeURIComponent(id)}`, {
    method: "DELETE",
    headers: CSRF_HEADERS,
  });
  if (!response.ok && response.status !== 404) {
    throw await failureOf(response);
  }
}

// Where a consent stands; a 404 is one that ran out or was never this user's.
export async function fetchConsent(id: string): Promise<ConsentOutcome> {
  const response = await reach(`${CONSENT_PENDING_ENDPOINT}/${encodeURIComponent(id)}`);
  if (response.status === 404) {
    return { status: "gone" };
  }
  if (!response.ok) {
    throw await failureOf(response);
  }
  return consentOutcomeSchema.parse(await response.json());
}

function foundOf(found: Found): FoundServer {
  return {
    provider: found.provider,
    kind: found.kind,
    target: found.target,
    credentialKind: found.credentialKind,
    host: found.host,
    oauthAvailable: found.oauthAvailable,
  };
}

// A request the network could not carry reads as unavailable.
async function reach(url: string, init?: RequestInit): Promise<Response> {
  try {
    return await fetch(url, init);
  } catch {
    throw new AccountsError("unavailable");
  }
}

async function send(method: "POST" | "PUT", url: string, body: object): Promise<Response> {
  const response = await reach(url, {
    method,
    headers: { "content-type": "application/json", ...CSRF_HEADERS },
    body: JSON.stringify(body),
  });
  if (response.ok) {
    return response;
  }
  throw await failureOf(response);
}

// The body names the refusal; a 429 says how long to wait; anything
// unnamed reads as unavailable.
async function failureOf(response: Response): Promise<AccountsError> {
  if (response.status === 429) {
    return new AccountsError("rate_limited", retryAfterSeconds(response));
  }
  const body: unknown = await response.json().catch(() => null);
  const parsed = errorBodySchema.safeParse(body);
  const code = parsed.success ? parsed.data.error : "";
  return new AccountsError(NAMED_FAILURES.find((named) => named === code) ?? "unavailable");
}
