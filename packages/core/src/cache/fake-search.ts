// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailHeader } from "../jmap/schemas";
import { z } from "../schema";
import { MethodError } from "./fake-wire";

// One FilterCondition of RFC 8621 section 4.4.1 with the properties a
// window or a search names; an operator or any other property is refused.
const conditionSchema = z.strictObject({
  inMailbox: z.optional(z.string()),
  inMailboxOtherThan: z.optional(z.array(z.string())),
  hasKeyword: z.optional(z.string()),
  notKeyword: z.optional(z.string()),
  after: z.optional(z.string()),
  before: z.optional(z.string()),
  hasAttachment: z.optional(z.boolean()),
  text: z.optional(z.string()),
  from: z.optional(z.string()),
});

type Condition = z.infer<typeof conditionSchema>;

// RFC 8620 section 1.4: a UTCDate ends in Z.
const utcDateSchema = z.iso.datetime();

// What a mailbox window names; a filter with anything else, or without
// a mailbox, is a search.
const WINDOW_KEYS = new Set(["inMailbox", "hasKeyword", "notKeyword"]);

const requestSchema = z.object({
  methodCalls: z.array(z.tuple([z.string(), z.record(z.string(), z.unknown()), z.string()])),
});

// A quoted run or a run of anything but white space.
const TERM = /"([^"]*)"|(\S+)/gu;

export function isSearch(filter: unknown): boolean {
  if (typeof filter !== "object" || filter === null) {
    return true;
  }
  const keys = Object.keys(filter);
  return !keys.includes("inMailbox") || keys.some((key) => !WINDOW_KEYS.has(key));
}

// Whether a request body carries a search.
export function asksSearch(body: unknown): boolean {
  const request = requestSchema.safeParse(body);
  return (
    request.success &&
    request.data.methodCalls.some(
      ([name, args]) => name === "Email/query" && isSearch(args["filter"]),
    )
  );
}

// The terms of a text, lower case: each word, a quoted run as one.
function termsOf(text: string): string[] {
  return [...text.matchAll(TERM)]
    .map((match) => (match[1] ?? match[2] ?? "").toLowerCase())
    .filter((term) => term !== "");
}

function sendersOf(row: EmailHeader): string[] {
  return (row.from ?? []).flatMap((address) => [address.name ?? "", address.email]);
}

// Every term is part of one of the fields, as a substring in any case.
function holdsAll(fields: readonly string[], text: string): boolean {
  const lower = fields.map((field) => field.toLowerCase());
  return termsOf(text).every((term) => lower.some((field) => field.includes(term)));
}

type Test = (row: EmailHeader) => boolean;

function mailboxTests(condition: Condition): Test[] {
  const { inMailbox, inMailboxOtherThan } = condition;
  return [
    ...(inMailbox === undefined ? [] : [(row: EmailHeader) => inMailbox in row.mailboxIds]),
    ...(inMailboxOtherThan === undefined
      ? []
      : [
          (row: EmailHeader) =>
            Object.keys(row.mailboxIds).some((id) => !inMailboxOtherThan.includes(id)),
        ]),
  ];
}

function keywordTests(condition: Condition): Test[] {
  const { hasKeyword, notKeyword, hasAttachment } = condition;
  return [
    ...(hasKeyword === undefined ? [] : [(row: EmailHeader) => hasKeyword in row.keywords]),
    ...(notKeyword === undefined ? [] : [(row: EmailHeader) => !(notKeyword in row.keywords)]),
    ...(hasAttachment === undefined
      ? []
      : [(row: EmailHeader) => row.hasAttachment === hasAttachment]),
  ];
}

function dateTests(condition: Condition): Test[] {
  const { after, before } = condition;
  if (
    [after, before].some((date) => date !== undefined && !utcDateSchema.safeParse(date).success)
  ) {
    throw new MethodError("invalidArguments");
  }
  return [
    ...(after === undefined
      ? []
      : [(row: EmailHeader) => Date.parse(row.receivedAt) >= Date.parse(after)]),
    ...(before === undefined
      ? []
      : [(row: EmailHeader) => Date.parse(row.receivedAt) < Date.parse(before)]),
  ];
}

function textTests(condition: Condition): Test[] {
  const { text, from } = condition;
  return [
    ...(text === undefined
      ? []
      : [
          (row: EmailHeader) => holdsAll([row.subject ?? "", ...sendersOf(row), row.preview], text),
        ]),
    ...(from === undefined ? [] : [(row: EmailHeader) => holdsAll(sendersOf(row), from)]),
  ];
}

// The emails a filter keeps: every condition it names holds; an absent
// or null filter keeps every email (RFC 8620 section 5.5).
export function matcherOf(filter: unknown): Test {
  const read = z.nullish(conditionSchema).safeParse(filter);
  if (!read.success) {
    throw new MethodError("unsupportedFilter");
  }
  const condition = read.data ?? {};
  const tests = [
    ...mailboxTests(condition),
    ...keywordTests(condition),
    ...dateTests(condition),
    ...textTests(condition),
  ];
  return (row) => tests.every((test) => test(row));
}
