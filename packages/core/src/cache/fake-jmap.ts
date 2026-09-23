// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailHeader, Mailbox } from "../jmap/schemas";
import { CORE_CAPABILITY, HULIHO_CAPABILITY, MAIL_CAPABILITY } from "../jmap/schemas";
import { z } from "../schema";
import { ACCOUNT, FakeMethods, MethodError, UPSTREAM } from "./fake-methods";
import type { Args, Change, Invocation, Kind, Type } from "./fake-methods";

export { ACCOUNT, UPSTREAM } from "./fake-methods";

const JSON_TYPE = "application/json";
const PROBLEM_TYPE = "application/problem+json";
const SESSION_URL = `/api/jmap/${ACCOUNT}/session`;
const API_URL = `/api/jmap/${ACCOUNT}`;
const KNOWN_CAPABILITIES = new Set([CORE_CAPABILITY, MAIL_CAPABILITY, HULIHO_CAPABILITY]);

function urlOf(input: RequestInfo | URL): string {
  if (typeof input === "string") {
    return input;
  }
  return input instanceof URL ? input.href : input.url;
}

function bodyOf(init: RequestInit): unknown {
  if (typeof init.body !== "string") {
    return null;
  }
  const body: unknown = JSON.parse(init.body);
  return body;
}

const requestSchema = z.object({
  using: z.array(z.string()),
  methodCalls: z.array(z.tuple([z.string(), z.record(z.string(), z.unknown()), z.string()])),
});

const referenceSchema = z.object({ resultOf: z.string(), name: z.string(), path: z.string() });

export function mailbox(id: string, role: string | null = null): Mailbox {
  return {
    id,
    name: id,
    parentId: null,
    role,
    sortOrder: 10,
    totalEmails: 0,
    unreadEmails: 0,
    totalThreads: 0,
    unreadThreads: 0,
    myRights: {
      mayReadItems: true,
      mayAddItems: false,
      mayRemoveItems: false,
      maySetSeen: false,
      maySetKeywords: false,
      mayCreateChild: false,
      mayRename: false,
      mayDelete: false,
      maySubmit: false,
    },
    isSubscribed: true,
    syncedEmails: 0,
  };
}

interface EmailFacts {
  threadId?: string;
  mailboxIds?: readonly string[];
  keywords?: readonly string[];
  receivedAt: string;
}

export function email(id: string, facts: EmailFacts): EmailHeader {
  return {
    id,
    blobId: id,
    threadId: facts.threadId ?? `t-${id}`,
    mailboxIds: Object.fromEntries((facts.mailboxIds ?? ["inbox"]).map((box) => [box, true])),
    keywords: Object.fromEntries((facts.keywords ?? []).map((word) => [word, true])),
    size: 1000,
    receivedAt: facts.receivedAt,
    messageId: [`${id}@example.test`],
    inReplyTo: null,
    references: null,
    sender: null,
    from: [{ name: "Sanne", email: "sanne@example.test" }],
    to: [{ name: null, email: "mo@example.test" }],
    cc: null,
    bcc: null,
    replyTo: null,
    subject: `Message ${id}`,
    sentAt: null,
    hasAttachment: false,
    preview: `Body of ${id}.`,
  };
}

// A UTCDate `seconds` into the fixture's day.
export function at(seconds: number): string {
  return new Date(Date.UTC(2026, 0, 1, 0, 0, seconds)).toISOString().replace(".000", "");
}

export function json(status: number, body: unknown, type = JSON_TYPE): Response {
  return new Response(JSON.stringify(body), { status, headers: { "content-type": type } });
}

export interface Recorded {
  method: string;
  url: string;
  headers: Headers;
  body: unknown;
}

// A JMAP server behind the proxy's routes, held in memory: the objects,
// a change log with a horizon, the limits of the session object and a
// queue of canned answers that go out before any request is read.
export class FakeJmap {
  readonly mailboxes = new Map<string, Mailbox>();
  readonly emails = new Map<string, EmailHeader>();
  readonly requests: Recorded[] = [];
  readonly queue: Response[] = [];
  changes: Change[] = [];
  sequence = 0;
  horizon = 0;
  maxObjectsInGet = 500;
  maxCallsInRequest = 16;
  queryLimit = 200;
  changesCap = Number.POSITIVE_INFINITY;
  vendor = true;
  sessionState = "s1";

  readonly fetch: typeof fetch = (input, init) =>
    Promise.resolve(this.handle(urlOf(input), init ?? {}));

  // The POST bodies sent so far, as the calls they carried.
  posted(): Invocation[][] {
    return this.requests
      .filter((request) => request.method === "POST")
      .map((request) => requestSchema.parse(request.body).methodCalls);
  }

  putMailbox(row: Mailbox): void {
    this.note("Mailbox", row.id, this.mailboxes.has(row.id) ? "updated" : "created");
    this.mailboxes.set(row.id, row);
  }

  removeMailbox(id: string): void {
    this.note("Mailbox", id, "destroyed");
    this.mailboxes.delete(id);
  }

  addEmail(row: EmailHeader): void {
    const known = [...this.emails.values()].some((held) => held.threadId === row.threadId);
    this.note("Email", row.id, "created");
    this.note("Thread", row.threadId, known ? "updated" : "created", false);
    this.emails.set(row.id, row);
  }

  // A keyword or mailbox change is an update of the email alone.
  amend(id: string, facts: Pick<Partial<EmailHeader>, "keywords" | "mailboxIds">): void {
    const held = this.emails.get(id);
    if (held === undefined) {
      throw new Error(`no email ${id}`);
    }
    this.note("Email", id, "updated");
    this.emails.set(id, { ...held, ...facts });
  }

  destroyEmail(id: string): void {
    const held = this.emails.get(id);
    if (held === undefined) {
      throw new Error(`no email ${id}`);
    }
    this.emails.delete(id);
    const left = [...this.emails.values()].some((row) => row.threadId === held.threadId);
    this.note("Email", id, "destroyed");
    this.note("Thread", held.threadId, left ? "updated" : "destroyed", false);
  }

  // The log up to now is gone: a client behind it cannot calculate changes.
  forget(): void {
    this.changes = [];
    this.horizon = this.sequence;
  }

  private note(type: Type, id: string, kind: Kind, fresh = true): void {
    if (fresh) {
      this.sequence += 1;
    }
    this.changes.push({ sequence: this.sequence, type, id, kind });
  }

  private handle(url: string, init: RequestInit): Response {
    const body = bodyOf(init);
    const headers = new Headers(init.headers);
    this.requests.push({ method: init.method ?? "GET", url, headers, body });
    const canned = this.queue.shift();
    if (canned !== undefined) {
      return canned;
    }
    switch (url) {
      case SESSION_URL:
        return json(200, this.session());
      case API_URL:
        return this.guarded(headers, body);
      default:
        return json(404, { error: "not_found" });
    }
  }

  // The proxy's own checks on a POST: the header every mutation carries
  // and the JSON content type.
  private guarded(headers: Headers, body: unknown): Response {
    if (headers.get("x-requested-with") !== "huliho") {
      return json(403, { error: "missing_csrf_header" });
    }
    if (headers.get("content-type") !== JSON_TYPE) {
      return json(400, { error: "invalid_request" });
    }
    return this.answer(body);
  }

  // The session object as the proxy serves it for the account.
  session(): Args {
    const core = {
      maxSizeUpload: 0,
      maxConcurrentUpload: 0,
      maxSizeRequest: 1_048_576,
      maxConcurrentRequests: 4,
      maxCallsInRequest: this.maxCallsInRequest,
      maxObjectsInGet: this.maxObjectsInGet,
      maxObjectsInSet: 0,
      collationAlgorithms: ["i;unicode-casemap"],
    };
    const vendor = this.vendor ? { [HULIHO_CAPABILITY]: {} } : {};
    return {
      capabilities: { [CORE_CAPABILITY]: core, [MAIL_CAPABILITY]: {}, ...vendor },
      accounts: {
        [UPSTREAM]: {
          name: "sanne@example.test",
          isPersonal: true,
          isReadOnly: true,
          accountCapabilities: { [MAIL_CAPABILITY]: {}, ...vendor },
        },
      },
      primaryAccounts: { [MAIL_CAPABILITY]: UPSTREAM },
      username: "sanne@example.test",
      apiUrl: `/api/jmap/${ACCOUNT}`,
      downloadUrl: `/api/jmap/${ACCOUNT}/download/{accountId}/{blobId}/{name}?type={type}`,
      uploadUrl: `/api/jmap/${ACCOUNT}/upload/{accountId}`,
      eventSourceUrl: `/api/jmap/${ACCOUNT}/events?types={types}&closeafter={closeafter}&ping={ping}`,
      state: this.sessionState,
    };
  }

  // One Request object: the capabilities checked, every call run in
  // order with its references resolved (RFC 8620 sections 3.3 and 3.7).
  private answer(body: unknown): Response {
    const request = requestSchema.safeParse(body);
    if (!request.success) {
      return json(400, { type: "urn:ietf:params:jmap:error:notRequest" }, PROBLEM_TYPE);
    }
    const { using, methodCalls } = request.data;
    if (using.some((name) => !KNOWN_CAPABILITIES.has(name))) {
      return json(400, { type: "urn:ietf:params:jmap:error:unknownCapability" }, PROBLEM_TYPE);
    }
    if (methodCalls.length > this.maxCallsInRequest) {
      const problem = { type: "urn:ietf:params:jmap:error:limit", limit: "maxCallsInRequest" };
      return json(400, problem, PROBLEM_TYPE);
    }
    const methods = new FakeMethods(this, using.includes(HULIHO_CAPABILITY));
    const responses: Invocation[] = [];
    for (const call of methodCalls) {
      responses.push(run(methods, responses, call));
    }
    return json(200, { methodResponses: responses, sessionState: this.sessionState });
  }
}

function run(
  methods: FakeMethods,
  responses: readonly Invocation[],
  [name, args, id]: Invocation,
): Invocation {
  try {
    return [name, methods.dispatch(name, resolve(args, responses)), id];
  } catch (error) {
    if (error instanceof MethodError) {
      return ["error", { type: error.type }, id];
    }
    throw error;
  }
}

// Every `#name` argument replaced by what its reference points at.
function resolve(args: Args, responses: readonly Invocation[]): Args {
  const resolved: [string, unknown][] = [];
  for (const [key, value] of Object.entries(args)) {
    if (!key.startsWith("#")) {
      resolved.push([key, value]);
      continue;
    }
    const ref = referenceSchema.safeParse(value);
    const found = responses.find(([, , callId]) => callId === ref.data?.resultOf);
    if (!ref.success || found === undefined || found[0] !== ref.data.name) {
      throw new MethodError("invalidResultReference");
    }
    resolved.push([key.slice(1), pointer(found[1], ref.data.path.split("/").slice(1))]);
  }
  return Object.fromEntries(resolved);
}

// A JSON pointer with the `*` of RFC 8620 section 3.7.
function pointer(value: unknown, steps: readonly string[]): unknown {
  const [step, ...rest] = steps;
  if (step === undefined) {
    return value;
  }
  if (Array.isArray(value)) {
    const items: unknown[] = value;
    if (step === "*") {
      return items.flatMap((item) => {
        const walked = pointer(item, rest);
        return Array.isArray(walked) ? (walked as unknown[]) : [walked];
      });
    }
    return pointer(items.at(Number(step)), rest);
  }
  if (typeof value === "object" && value !== null) {
    const entry = new Map<string, unknown>(Object.entries(value)).get(step);
    if (entry === undefined) {
      throw new MethodError("invalidResultReference");
    }
    return pointer(entry, rest);
  }
  throw new MethodError("invalidResultReference");
}
