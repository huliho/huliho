// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The JMAP methods the cache calls, answered from a corpus: the query
// with its anchor, collapse and keyword filters, the gets by id or by
// reference with the body of a message, the writes, the changes since a
// state, the mailbox list and the quotas.

import { bodyOf, bodyPropertiesOf } from "./mail-bodies";
import { UPSTREAM, emailObject } from "./mail-corpus";
import type { Corpus, CorpusEmail } from "./mail-corpus";
import { emailSet } from "./mail-writes";

type Args = Record<string, unknown>;
export type Invocation = [string, Args, string];

// One change the server logged, in the order they happened.
interface Change {
  sequence: number;
  type: "Email" | "Thread" | "Mailbox";
  id: string;
  kind: "created" | "updated" | "destroyed";
}

// The server behind the routes: the corpus, the mailboxes it lists and
// a change log whose length is every state string, plus the knobs a
// test turns on its body requests and its writes.
export interface MailServer {
  corpus: Corpus;
  mailboxes: Record<string, unknown>[];
  changes: Change[];
  // The query's cap on one page; the client asks a hundred.
  queryLimit: number;
  // The messages Email/get answers as not found.
  gone: Set<string>;
  // The SetError type an update of the message answers.
  refused: Map<string, string>;
  // The arguments of every Email/set, in order.
  sets: Args[];
  // The cap of every body ask, in order.
  asked: number[];
  // Whether a body request waits until released, and the ones waiting.
  holdBodies: boolean;
  heldBodies: (() => void)[];
  // How many body requests still answer the limit problem.
  limitBodies: number;
  // The account's quotas, null for an account without the capability.
  quotas: Record<string, unknown>[] | null;
}

export function serverFor(
  corpus: Corpus,
  mailboxes: Record<string, unknown>[],
  quotas: Record<string, unknown>[] | null = null,
): MailServer {
  return {
    corpus,
    mailboxes,
    changes: [],
    queryLimit: 200,
    gone: new Set(),
    refused: new Map(),
    sets: [],
    asked: [],
    holdBodies: false,
    heldBodies: [],
    limitBodies: 0,
    quotas,
  };
}

// Logs a message that arrived, so the next poll sees it.
export function noteArrival(server: MailServer, id: string, threadId: string): void {
  const sequence = server.changes.length + 1;
  server.changes.push({ sequence, type: "Email", id, kind: "created" });
  server.changes.push({ sequence, type: "Thread", id: threadId, kind: "created" });
}

// Whether a request asks for a message's body values.
export function asksBody(calls: readonly Invocation[]): boolean {
  return calls.some(([name, args]) => name === "Email/get" && args["fetchHTMLBodyValues"] === true);
}

// Lets every held body request through and holds no more.
export function releaseBodies(server: MailServer): void {
  server.holdBodies = false;
  for (const go of server.heldBodies.splice(0)) {
    go();
  }
}

function stateOf(server: MailServer): string {
  return String(server.changes.length);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isStringList(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

function stringsAt(record: Record<string, unknown>, key: string): string[] {
  const value = new Map(Object.entries(record)).get(key);
  return isStringList(value) ? value : [];
}

// The answer a reference points into and the path it takes there.
function referenced(
  reference: unknown,
  prior: Map<string, Args>,
): { answer: Record<string, unknown>; path: string } {
  if (!isRecord(reference) || typeof reference["resultOf"] !== "string") {
    throw new Error("invalidResultReference");
  }
  const answer = prior.get(reference["resultOf"]);
  const path = reference["path"];
  if (answer === undefined || typeof path !== "string") {
    throw new Error("invalidResultReference");
  }
  return { answer, path };
}

// The objects of a list answer, for the paths that walk it.
function listOf(answer: Record<string, unknown>): Record<string, unknown>[] {
  const list = answer["list"];
  return Array.isArray(list) ? list.filter((item) => isRecord(item)) : [];
}

// The ids a reference points at, for the paths the cache uses
// (RFC 8620 section 3.7): the query's ids, the threads of a list of
// emails, the emails of a list of threads, the created and the updated.
function resolve(reference: unknown, prior: Map<string, Args>): string[] {
  const { answer, path } = referenced(reference, prior);
  switch (path) {
    case "/ids":
    case "/created":
    case "/updated":
      return stringsAt(answer, path.slice(1));
    case "/list/*/threadId":
      return listOf(answer).flatMap((item) => stringsAt({ ids: [item["threadId"]] }, "ids"));
    case "/list/*/emailIds":
      return listOf(answer).flatMap((item) => stringsAt(item, "emailIds"));
    default:
      throw new Error("invalidResultReference");
  }
}

// The ids a get names, given outright or by reference.
function idsOf(args: Args, prior: Map<string, Args>): string[] {
  if ("#ids" in args) {
    return resolve(args["#ids"], prior);
  }
  return stringsAt(args, "ids");
}

function picked(object: Record<string, unknown>, properties: unknown): Record<string, unknown> {
  if (!isStringList(properties)) {
    return object;
  }
  const fields = new Map(Object.entries(object));
  return Object.fromEntries(["id", ...properties].map((name) => [name, fields.get(name)]));
}

// The Email object with its body properties where the request asks for
// values.
function served(server: MailServer, message: CorpusEmail, args: Args): Record<string, unknown> {
  const object = emailObject(message);
  if (args["fetchHTMLBodyValues"] !== true && args["fetchTextBodyValues"] !== true) {
    return object;
  }
  const body = bodyOf(message, server.corpus.bodies);
  const cap = args["maxBodyValueBytes"];
  if (typeof cap === "number") {
    server.asked.push(cap);
  }
  const asked = {
    fetchTextBodyValues: args["fetchTextBodyValues"] === true,
    fetchHTMLBodyValues: args["fetchHTMLBodyValues"] === true,
    ...(typeof cap === "number" ? { maxBodyValueBytes: cap } : {}),
  };
  return { ...object, ...bodyPropertiesOf(message, body, asked) };
}

function emailGet(server: MailServer, args: Args, prior: Map<string, Args>): Args {
  const ids = idsOf(args, prior);
  const found = ids.filter((id) => !server.gone.has(id) && server.corpus.emails.has(id));
  const list = found.flatMap((id) => {
    const message = server.corpus.emails.get(id);
    return message === undefined ? [] : [picked(served(server, message, args), args["properties"])];
  });
  const notFound = ids.filter((id) => !found.includes(id));
  return { accountId: UPSTREAM, state: stateOf(server), list, notFound };
}

function threadGet(server: MailServer, args: Args, prior: Map<string, Args>): Args {
  const list = idsOf(args, prior).flatMap((id) => {
    const emailIds = server.corpus.threads.get(id);
    return emailIds === undefined ? [] : [{ id, emailIds }];
  });
  return { accountId: UPSTREAM, state: stateOf(server), list, notFound: [] };
}

// RFC 8620 section 5.5: the anchor wins over the position.
function startOf(args: Args, ids: readonly string[]): number {
  const anchor = args["anchor"];
  if (typeof anchor !== "string") {
    return typeof args["position"] === "number" ? args["position"] : 0;
  }
  const at = ids.indexOf(anchor);
  if (at < 0) {
    throw new Error("anchorNotFound");
  }
  const offset = typeof args["anchorOffset"] === "number" ? args["anchorOffset"] : 0;
  return Math.max(0, at + offset);
}

// The keywords a filter may name, as a corpus message holds them.
const HELD_KEYWORDS = new Map<string, (message: CorpusEmail) => boolean>([
  ["$seen", (message) => message.seen],
  ["$flagged", (message) => message.flagged],
]);

// The one filter shape the cache asks (RFC 8621 section 4.4.1): a
// mailbox, with a keyword it must have or lack.
function filterOf(server: MailServer, filter: unknown): (id: string) => boolean {
  const fields = new Map(Object.entries(isRecord(filter) ? filter : {}));
  const mailboxId = fields.get("inMailbox");
  const known = new Set(["inMailbox", "hasKeyword", "notKeyword"]);
  if (typeof mailboxId !== "string" || [...fields.keys()].some((key) => !known.has(key))) {
    throw new Error("unsupportedFilter");
  }
  const has = HELD_KEYWORDS.get(String(fields.get("hasKeyword")));
  const lacks = HELD_KEYWORDS.get(String(fields.get("notKeyword")));
  return (id) => {
    const message = server.corpus.emails.get(id);
    return (
      message !== undefined &&
      (has === undefined || has(message)) &&
      (lacks === undefined || !lacks(message))
    );
  };
}

function emailQuery(server: MailServer, args: Args): Args {
  const filter = args["filter"];
  const keep = filterOf(server, filter);
  const mailboxId = String(
    new Map(Object.entries(isRecord(filter) ? filter : {})).get("inMailbox"),
  );
  const list = server.corpus.lists.get(mailboxId) ?? { ids: [], exemplars: [] };
  const ids = (args["collapseThreads"] === true ? list.exemplars : list.ids).filter(keep);
  const position = startOf(args, ids);
  const asked = typeof args["limit"] === "number" ? args["limit"] : server.queryLimit;
  const limit = Math.min(asked, server.queryLimit);
  const answer: Args = {
    accountId: UPSTREAM,
    queryState: stateOf(server),
    canCalculateChanges: false,
    position,
    ids: ids.slice(position, position + limit),
  };
  if (args["calculateTotal"] === true) {
    answer["total"] = ids.length;
  }
  if (limit < asked) {
    answer["limit"] = limit;
  }
  return answer;
}

function changes(server: MailServer, type: Change["type"], args: Args): Args {
  const since = Number(args["sinceState"]);
  const rows = server.changes.filter((row) => row.type === type && row.sequence > since);
  const kinds = (kind: Change["kind"]): string[] => [
    ...new Set(rows.filter((row) => row.kind === kind).map((row) => row.id)),
  ];
  return {
    accountId: UPSTREAM,
    oldState: String(since),
    newState: stateOf(server),
    hasMoreChanges: false,
    created: kinds("created"),
    updated: kinds("updated"),
    destroyed: kinds("destroyed"),
    ...(type === "Mailbox" ? { updatedProperties: null } : {}),
  };
}

function mailboxGet(server: MailServer): Args {
  return { accountId: UPSTREAM, state: stateOf(server), list: server.mailboxes, notFound: [] };
}

// The quotas never move, so their state stands still (RFC 9425 section 4).
const QUOTA_STATE = "q1";

function quotaGet(server: MailServer): Args {
  if (server.quotas === null) {
    throw new Error("accountNotSupportedByMethod");
  }
  return { accountId: UPSTREAM, state: QUOTA_STATE, list: server.quotas, notFound: [] };
}

function quotaChanges(server: MailServer, args: Args): Args {
  if (server.quotas === null) {
    throw new Error("accountNotSupportedByMethod");
  }
  if (args["sinceState"] !== QUOTA_STATE) {
    throw new Error("cannotCalculateChanges");
  }
  return {
    accountId: UPSTREAM,
    oldState: QUOTA_STATE,
    newState: QUOTA_STATE,
    hasMoreChanges: false,
    created: [],
    updated: [],
    destroyed: [],
  };
}

type Method = (server: MailServer, args: Args, prior: Map<string, Args>) => Args;

const METHODS = new Map<string, Method>([
  ["Mailbox/get", mailboxGet],
  ["Mailbox/changes", (server, args) => changes(server, "Mailbox", args)],
  ["Email/get", emailGet],
  ["Email/set", emailSet],
  ["Email/query", emailQuery],
  ["Email/changes", (server, args) => changes(server, "Email", args)],
  ["Thread/get", threadGet],
  ["Thread/changes", (server, args) => changes(server, "Thread", args)],
  ["Quota/get", quotaGet],
  ["Quota/changes", quotaChanges],
]);

function dispatch(server: MailServer, call: Invocation, prior: Map<string, Args>): Args {
  const [name, args] = call;
  const method = METHODS.get(name);
  if (method === undefined) {
    throw new Error("unknownMethod");
  }
  return method(server, args, prior);
}

// The method responses of one request, in order; a call that fails
// answers the error type the failure names.
export function answerCalls(server: MailServer, calls: readonly Invocation[]): Invocation[] {
  const prior = new Map<string, Args>();
  return calls.map((call) => {
    const [name, , id] = call;
    try {
      const answer = dispatch(server, call, prior);
      prior.set(id, answer);
      return [name, answer, id];
    } catch (error) {
      return ["error", { type: error instanceof Error ? error.message : "serverFail" }, id];
    }
  });
}
