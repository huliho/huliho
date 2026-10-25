// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The searches a client sends, answered over the corpus as RFC 8621
// section 4.4.1 has them: a text and a sender as terms, each a part of
// a field in any case, the dates, the attachment, the keywords and the
// scope. A filter without a mailbox spans every mailbox.

import type { Corpus, CorpusEmail } from "./mail-corpus";

type Args = Record<string, unknown>;
type Invocation = [string, Args, string];
type Test = (message: CorpusEmail) => boolean;

// What a mailbox window names; a filter with anything else, or without
// a mailbox, is a search.
const WINDOW_KEYS = new Set(["inMailbox", "hasKeyword", "notKeyword"]);

const KNOWN_KEYS = new Set([
  ...WINDOW_KEYS,
  "inMailboxOtherThan",
  "after",
  "before",
  "hasAttachment",
  "text",
  "from",
]);

// A quoted run or a run of anything but white space.
const TERM = /"([^"]*)"|(\S+)/gu;

// The keywords a corpus message can hold.
const HELD = new Map<string, Test>([
  ["$seen", (message) => message.seen],
  ["$flagged", (message) => message.flagged],
]);

function isRecord(value: unknown): value is Args {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isStringList(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

export function isSearch(filter: unknown): boolean {
  if (!isRecord(filter)) {
    return true;
  }
  const keys = Object.keys(filter);
  return !keys.includes("inMailbox") || keys.some((key) => !WINDOW_KEYS.has(key));
}

// Whether a request carries a search.
export function asksSearch(calls: readonly Invocation[]): boolean {
  return calls.some(([name, args]) => name === "Email/query" && isSearch(args["filter"]));
}

function termsOf(text: string): string[] {
  return [...text.matchAll(TERM)]
    .map((match) => (match[1] ?? match[2] ?? "").toLowerCase())
    .filter((term) => term !== "");
}

// Every term is part of one of the fields, in any case.
function holdsAll(fields: readonly string[], text: string): boolean {
  const lower = fields.map((field) => field.toLowerCase());
  return termsOf(text).every((term) => lower.some((field) => field.includes(term)));
}

function senderOf(message: CorpusEmail): string[] {
  return [message.from.name, message.from.email];
}

function holds(keyword: string, message: CorpusEmail): boolean {
  return HELD.get(keyword)?.(message) ?? false;
}

function stringOf(filter: Args, key: string): string | undefined {
  const value = new Map(Object.entries(filter)).get(key);
  if (value !== undefined && typeof value !== "string") {
    throw new Error("unsupportedFilter");
  }
  return value;
}

// A UTCDate is a date-time that ends in Z (RFC 8620 section 1.4).
function dateOf(filter: Args, key: string): number | undefined {
  const value = stringOf(filter, key);
  if (value === undefined) {
    return undefined;
  }
  const time = Date.parse(value);
  if (!value.includes("T") || !value.endsWith("Z") || Number.isNaN(time)) {
    throw new Error("invalidArguments");
  }
  return time;
}

function scopeTests(filter: Args): Test[] {
  const inMailbox = stringOf(filter, "inMailbox");
  const otherThan = filter["inMailboxOtherThan"];
  if (otherThan !== undefined && !isStringList(otherThan)) {
    throw new Error("unsupportedFilter");
  }
  return [
    ...(inMailbox === undefined ? [] : [(message: CorpusEmail) => message.mailboxId === inMailbox]),
    ...(otherThan === undefined
      ? []
      : [(message: CorpusEmail) => !otherThan.includes(message.mailboxId)]),
  ];
}

function flagTests(filter: Args): Test[] {
  const has = stringOf(filter, "hasKeyword");
  const lacks = stringOf(filter, "notKeyword");
  const attachment = filter["hasAttachment"];
  if (attachment !== undefined && typeof attachment !== "boolean") {
    throw new Error("unsupportedFilter");
  }
  return [
    ...(has === undefined ? [] : [(message: CorpusEmail) => holds(has, message)]),
    ...(lacks === undefined ? [] : [(message: CorpusEmail) => !holds(lacks, message)]),
    ...(attachment === undefined
      ? []
      : [(message: CorpusEmail) => message.attachment === attachment]),
  ];
}

function timeTests(filter: Args): Test[] {
  const after = dateOf(filter, "after");
  const before = dateOf(filter, "before");
  return [
    ...(after === undefined
      ? []
      : [(message: CorpusEmail) => Date.parse(message.receivedAt) >= after]),
    ...(before === undefined
      ? []
      : [(message: CorpusEmail) => Date.parse(message.receivedAt) < before]),
  ];
}

function textTests(filter: Args): Test[] {
  const text = stringOf(filter, "text");
  const from = stringOf(filter, "from");
  return [
    ...(text === undefined
      ? []
      : [
          (message: CorpusEmail) =>
            holdsAll([message.subject, ...senderOf(message), message.preview], text),
        ]),
    ...(from === undefined ? [] : [(message: CorpusEmail) => holdsAll(senderOf(message), from)]),
  ];
}

// The test a filter makes; an absent or null filter keeps every message
// (RFC 8620 section 5.5), an operator or an unknown property is refused.
function testOf(filter: unknown): Test {
  if (filter === undefined || filter === null) {
    return () => true;
  }
  if (!isRecord(filter) || Object.keys(filter).some((key) => !KNOWN_KEYS.has(key))) {
    throw new Error("unsupportedFilter");
  }
  const tests = [
    ...scopeTests(filter),
    ...flagTests(filter),
    ...timeTests(filter),
    ...textTests(filter),
  ];
  return (message) => tests.every((test) => test(message));
}

function newestFirst(a: CorpusEmail, b: CorpusEmail): number {
  if (a.receivedAt !== b.receivedAt) {
    return a.receivedAt < b.receivedAt ? 1 : -1;
  }
  return a.id < b.id ? 1 : -1;
}

// The ids a search answers, newest first, one per thread on request.
export function searchIds(corpus: Corpus, filter: unknown, collapse: boolean): string[] {
  const keeps = testOf(filter);
  const threads = new Set<string>();
  return [...corpus.emails.values()]
    .filter(keeps)
    .toSorted(newestFirst)
    .filter((message) => {
      if (!collapse) {
        return true;
      }
      const first = !threads.has(message.threadId);
      threads.add(message.threadId);
      return first;
    })
    .map((message) => message.id);
}
