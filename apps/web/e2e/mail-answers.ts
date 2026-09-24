// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The JMAP methods the cache calls, answered from a corpus: the query
// with its anchor and collapse, the gets by id or by reference, the
// changes since a state and the mailbox list.

import { UPSTREAM, emailObject } from "./mail-corpus";
import type { Corpus } from "./mail-corpus";

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
// a change log whose length is every state string.
export interface MailServer {
  corpus: Corpus;
  mailboxes: readonly object[];
  changes: Change[];
  // The query's cap on one page; the client asks a hundred.
  queryLimit: number;
}

export function serverFor(corpus: Corpus, mailboxes: readonly object[]): MailServer {
  return { corpus, mailboxes, changes: [], queryLimit: 200 };
}

// Logs a message that arrived, so the next poll sees it.
export function noteArrival(server: MailServer, id: string, threadId: string): void {
  const sequence = server.changes.length + 1;
  server.changes.push({ sequence, type: "Email", id, kind: "created" });
  server.changes.push({ sequence, type: "Thread", id: threadId, kind: "created" });
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

function emailGet(server: MailServer, args: Args, prior: Map<string, Args>): Args {
  const ids = idsOf(args, prior);
  const list = ids.flatMap((id) => {
    const message = server.corpus.emails.get(id);
    return message === undefined ? [] : [picked(emailObject(message), args["properties"])];
  });
  return { accountId: UPSTREAM, state: stateOf(server), list, notFound: [] };
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

function emailQuery(server: MailServer, args: Args): Args {
  const filter = args["filter"];
  const mailboxId = isRecord(filter) ? filter["inMailbox"] : undefined;
  if (typeof mailboxId !== "string") {
    throw new Error("unsupportedFilter");
  }
  const list = server.corpus.lists.get(mailboxId) ?? { ids: [], exemplars: [] };
  const ids = args["collapseThreads"] === true ? list.exemplars : list.ids;
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

function dispatch(server: MailServer, call: Invocation, prior: Map<string, Args>): Args {
  const [name, args] = call;
  switch (name) {
    case "Mailbox/get":
      return mailboxGet(server);
    case "Mailbox/changes":
      return changes(server, "Mailbox", args);
    case "Email/get":
      return emailGet(server, args, prior);
    case "Email/query":
      return emailQuery(server, args);
    case "Email/changes":
      return changes(server, "Email", args);
    case "Thread/get":
      return threadGet(server, args, prior);
    case "Thread/changes":
      return changes(server, "Thread", args);
    default:
      throw new Error("unknownMethod");
  }
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
