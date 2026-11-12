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
  QUOTA_CAPABILITY,
  SUBMISSION_CAPABILITY,
  coreLimitsSchema,
  problemSchema,
  responseSchema,
  sessionObjectSchema,
} from "./schemas";
import type { Invocation, SessionObject } from "./schemas";

const JMAP_ROUTE = "/api/jmap";

// The longest mailbox name where the account names none, in octets of
// UTF-8: the cap the bridge holds names to.
const NAME_BYTES_DEFAULT = 255;

const nameBytesSchema = z.int().check(z.positive());

const octetsSchema = z.int().check(z.nonnegative());

// The problem type of a request past one of the server's limits
// (RFC 8620 section 3.6.1).
const LIMIT_PROBLEM = "urn:ietf:params:jmap:error:limit";

// The limit the body layer enforces with a 413 before the proxy reads
// the request.
const BODY_LIMIT = "maxSizeRequest";

const stopCauseSchema = z.enum(["credentials", "connection"]);

// The error body of the API routes; the cause rides along on still_stopped.
const apiErrorSchema = z.object({ error: z.string(), cause: z.optional(stopCauseSchema) });

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

// What a server answers while the mail behind it is out of reach
// (RFC 8620 section 3.6.2); it reads as the network does, so the
// caller tries again.
export const SERVER_UNAVAILABLE = "serverUnavailable";

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

// What the account allows a folder (RFC 8621 section 1.3.1): a create
// at the top of the tree and the longest name in octets of UTF-8.
export interface MailboxLimits {
  mayCreateTopLevelMailbox: boolean;
  maxSizeMailboxName: number;
}

// What the account takes of files (RFC 8620 section 2 and RFC 8621
// section 1.3.1): the largest one, how many go up at once and what the
// attachments of one email may weigh together, null where it names none.
export interface UploadLimits {
  maxSizeUpload: number;
  maxConcurrentUpload: number;
  maxSizeAttachmentsPerEmail: number | null;
}

// What the client keeps of the session object.
export interface JmapSession {
  // The upstream account the mail capability names; every call carries it.
  accountId: string;
  apiUrl: string;
  // The template a blob's URL is made from (RFC 8620 section 6.2).
  downloadUrl: string;
  // The template a file is uploaded to (RFC 8620 section 6.1).
  uploadUrl: string;
  uploadLimits: UploadLimits;
  // True when the account sends: the identity and submission methods
  // answer (RFC 8621 section 1.3.2).
  submission: boolean;
  // True where the server writes a text part's flowed parameters with its
  // charset: the servers that name the vendor capability.
  flowedText: boolean;
  // True for an account that takes no write.
  readOnly: boolean;
  // The capabilities every request opts into (RFC 8620 section 3.3).
  using: string[];
  // True for a bridge account, whose mailboxes count their synced emails.
  firstSync: boolean;
  // True when the account answers the quota methods (RFC 9425 section 2.1).
  quota: boolean;
  maxCallsInRequest: number;
  maxObjectsInGet: number;
  maxObjectsInSet: number;
  mailboxLimits: MailboxLimits;
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
  // responses in the server's order. A `signal` that aborts gives the
  // request up.
  async request(calls: readonly MethodCall[], signal?: AbortSignal): Promise<Invocation[]> {
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
      ...(signal === undefined ? {} : { signal }),
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
// A request opts into every capability the server names that the client
// speaks, so a server that knows quota hears the quota calls.
function sessionOf(object: SessionObject): JmapSession {
  const accountId = new Map(Object.entries(object.primaryAccounts)).get(MAIL_CAPABILITY);
  if (accountId === undefined) {
    throw new JmapError("upstream");
  }
  const capabilities = new Map(Object.entries(object.capabilities));
  const vendor = capabilities.has(HULIHO_CAPABILITY);
  const core = capabilities.get(CORE_CAPABILITY);
  const limits = coreLimitsSchema.parse(core);
  const account = new Map(Object.entries(object.accounts)).get(accountId);
  const named = new Map(Object.entries(account?.accountCapabilities ?? {}));
  const spoken = [HULIHO_CAPABILITY, QUOTA_CAPABILITY, SUBMISSION_CAPABILITY].filter((name) =>
    capabilities.has(name),
  );
  const both = (name: string): boolean => capabilities.has(name) && named.has(name);
  return {
    accountId,
    apiUrl: object.apiUrl,
    downloadUrl: object.downloadUrl,
    uploadUrl: object.uploadUrl,
    uploadLimits: uploadLimitsOf(fieldsOf(core), fieldsOf(named.get(MAIL_CAPABILITY))),
    submission: both(SUBMISSION_CAPABILITY),
    flowedText: vendor,
    // An account the object does not describe takes no write.
    readOnly: account?.isReadOnly ?? true,
    using: [CORE_CAPABILITY, MAIL_CAPABILITY, ...spoken],
    firstSync: vendor,
    quota: both(QUOTA_CAPABILITY),
    maxCallsInRequest: limits.maxCallsInRequest,
    maxObjectsInGet: limits.maxObjectsInGet,
    maxObjectsInSet: limits.maxObjectsInSet,
    mailboxLimits: mailboxLimitsOf(fieldsOf(named.get(MAIL_CAPABILITY))),
    state: object.state,
  };
}

// The fields of a capability object, none for anything else.
function fieldsOf(capability: unknown): Map<string, unknown> {
  return new Map(
    typeof capability === "object" && capability !== null ? Object.entries(capability) : [],
  );
}

// The folder limits of the account's mail capability, each field read on
// its own: one that is absent or fails reads as no create at the top and
// NAME_BYTES_DEFAULT, as a null name size does.
function mailboxLimitsOf(fields: ReadonlyMap<string, unknown>): MailboxLimits {
  const bytes = nameBytesSchema.safeParse(fields.get("maxSizeMailboxName"));
  return {
    mayCreateTopLevelMailbox: fields.get("mayCreateTopLevelMailbox") === true,
    maxSizeMailboxName: bytes.success ? bytes.data : NAME_BYTES_DEFAULT,
  };
}

// One field as a count of octets, null where it is absent or fails.
function octets(fields: ReadonlyMap<string, unknown>, name: string): number | null {
  const read = octetsSchema.safeParse(fields.get(name));
  return read.success ? read.data : null;
}

// The file limits of the core capability and the account's mail
// capability, each field read on its own. An upload limit that is
// absent or fails reads as 0, so no file goes up; an attachment total
// reads as none.
function uploadLimitsOf(
  core: ReadonlyMap<string, unknown>,
  mail: ReadonlyMap<string, unknown>,
): UploadLimits {
  return {
    maxSizeUpload: octets(core, "maxSizeUpload") ?? 0,
    maxConcurrentUpload: octets(core, "maxConcurrentUpload") ?? 0,
    maxSizeAttachmentsPerEmail: octets(mail, "maxSizeAttachmentsPerEmail"),
  };
}

// A request the network could not carry reads as unavailable; one its
// caller gave up rejects with the caller's own reason.
async function reach(url: string, init?: RequestInit): Promise<Response> {
  try {
    return await fetch(url, init);
  } catch (error) {
    if (init?.signal?.aborted === true) {
      throw error;
    }
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
