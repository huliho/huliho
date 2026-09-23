// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { StopCause } from "../accounts";
import { CSRF_HEADERS } from "../http";
import { z } from "../schema";
import {
  CORE_CAPABILITY,
  HULIHO_CAPABILITY,
  MAIL_CAPABILITY,
  coreLimitsSchema,
  problemSchema,
  responseSchema,
  sessionObjectSchema,
} from "./schemas";
import type { Invocation, SessionObject } from "./schemas";

const JMAP_ROUTE = "/api/jmap";

// The problem type of a request past one of the server's limits
// (RFC 8620 section 3.6.1).
const LIMIT_PROBLEM = "urn:ietf:params:jmap:error:limit";

// The limit the body layer enforces with a 413 before the proxy reads
// the request.
const BODY_LIMIT = "maxSizeRequest";

const stopCauseSchema = z.enum(["credentials", "connection"]);

// The error body of the API routes; the cause rides along on still_stopped.
const apiErrorSchema = z.object({ error: z.string(), cause: stopCauseSchema.optional() });

// How a request fails before any method ran, in the words the routes use.
export type JmapFailureCode =
  | "stopped"
  | "credentials"
  | "upstream"
  | "limit"
  | "not_found"
  | "unauthenticated"
  | "unavailable";

export class JmapError extends Error {
  readonly code: JmapFailureCode;
  // The cause of a stopped account, null for every other code.
  readonly stopCause: StopCause | null;
  // The name of the limit a request passed, null for every other code.
  readonly limit: string | null;

  constructor(code: JmapFailureCode, detail: { stopCause?: StopCause; limit?: string } = {}) {
    super(`jmap request failed: ${code}`);
    this.name = "JmapError";
    this.code = code;
    this.stopCause = detail.stopCause ?? null;
    this.limit = detail.limit ?? null;
  }
}

// A method that answered an error (RFC 8620 section 3.6.2) where the
// caller needed its value.
export class MethodFailure extends Error {
  readonly type: string;
  readonly call: string;

  constructor(type: string, call: string) {
    super(`method call ${call} failed: ${type}`);
    this.name = "MethodFailure";
    this.type = type;
    this.call = call;
  }
}

// What the client keeps of the session object.
export interface JmapSession {
  // The upstream account the mail capability names; every call carries it.
  accountId: string;
  apiUrl: string;
  // The capabilities every request opts into (RFC 8620 section 3.3).
  using: string[];
  // True for a bridge account, whose mailboxes count their synced emails.
  firstSync: boolean;
  maxCallsInRequest: number;
  maxObjectsInGet: number;
  state: string;
}

export interface MethodCall {
  name: string;
  arguments: Record<string, unknown>;
  id: string;
}

// One account's endpoint on the proxy.
export class JmapClient {
  // The Huliho account id, which is also the cache key.
  readonly accountId: string;
  private held: JmapSession | null = null;

  constructor(accountId: string) {
    this.accountId = accountId;
  }

  // The session object as the proxy serves it, fetched once and again
  // after a response says it moved.
  async session(): Promise<JmapSession> {
    if (this.held !== null) {
      return this.held;
    }
    const response = await reach(`${JMAP_ROUTE}/${encodeURIComponent(this.accountId)}/session`);
    if (!response.ok) {
      throw await failureOf(response);
    }
    this.held = sessionOf(sessionObjectSchema.parse(await response.json()));
    return this.held;
  }

  // One Request object (RFC 8620 section 3.3) in one round trip; the
  // responses in the server's order.
  async request(calls: readonly MethodCall[]): Promise<Invocation[]> {
    const session = await this.session();
    if (calls.length > session.maxCallsInRequest) {
      throw new JmapError("limit", { limit: "maxCallsInRequest" });
    }
    const body = {
      using: session.using,
      methodCalls: calls.map((call) => [call.name, call.arguments, call.id]),
    };
    const response = await reach(session.apiUrl, {
      method: "POST",
      headers: { "content-type": "application/json", ...CSRF_HEADERS },
      body: JSON.stringify(body),
    });
    if (!response.ok) {
      throw await failureOf(response);
    }
    const parsed = responseSchema.parse(await response.json());
    // RFC 8620 section 3.4: a moved sessionState means the session object changed.
    if (parsed.sessionState !== session.state) {
      this.held = null;
    }
    return parsed.methodResponses;
  }
}

// An upstream that names no mail account cannot be read through the proxy.
function sessionOf(object: SessionObject): JmapSession {
  const accountId = new Map(Object.entries(object.primaryAccounts)).get(MAIL_CAPABILITY);
  if (accountId === undefined) {
    throw new JmapError("upstream");
  }
  const capabilities = new Map(Object.entries(object.capabilities));
  const vendor = capabilities.has(HULIHO_CAPABILITY);
  const core = coreLimitsSchema.parse(capabilities.get(CORE_CAPABILITY));
  return {
    accountId,
    apiUrl: object.apiUrl,
    using: vendor
      ? [CORE_CAPABILITY, MAIL_CAPABILITY, HULIHO_CAPABILITY]
      : [CORE_CAPABILITY, MAIL_CAPABILITY],
    firstSync: vendor,
    maxCallsInRequest: core.maxCallsInRequest,
    maxObjectsInGet: core.maxObjectsInGet,
    state: object.state,
  };
}

// A request the network could not carry reads as unavailable.
async function reach(url: string, init?: RequestInit): Promise<Response> {
  try {
    return await fetch(url, init);
  } catch {
    throw new JmapError("unavailable");
  }
}

// The refusal by its body: the limit problem, the routes' error word or
// the body layer's 413; anything unnamed reads as unavailable.
async function failureOf(response: Response): Promise<JmapError> {
  if (response.status === 413) {
    return new JmapError("limit", { limit: BODY_LIMIT });
  }
  const body: unknown = await response.json().catch(() => null);
  const problem = problemSchema.safeParse(body);
  if (problem.success && problem.data.type === LIMIT_PROBLEM) {
    const { limit } = problem.data;
    return limit === undefined ? new JmapError("limit") : new JmapError("limit", { limit });
  }
  const named = apiErrorSchema.safeParse(body);
  return named.success ? namedFailure(named.data) : new JmapError("unavailable");
}

function namedFailure(body: z.infer<typeof apiErrorSchema>): JmapError {
  switch (body.error) {
    case "still_stopped":
      return body.cause === undefined
        ? new JmapError("unavailable")
        : new JmapError("stopped", { stopCause: body.cause });
    case "upstream_credentials":
      return new JmapError("credentials");
    case "not_found":
      return new JmapError("not_found");
    case "unauthenticated":
      return new JmapError("unauthenticated");
    case "upstream_unreachable":
    case "upstream_failed":
    case "upstream_unsupported":
    case "upstream_insecure":
      return new JmapError("upstream");
    default:
      return new JmapError("unavailable");
  }
}
