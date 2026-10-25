// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailFilter } from "../jmap/calls";
import type { Mailbox } from "../jmap/schemas";
import { z } from "../schema";
import { SEEN } from "./counts";
import type { Scope } from "./scope";

// A search as the route carries it: every mailbox but Junk and Trash
// unless `in` names a mailbox or `all`. The dates are calendar days with
// `before` the first day the range leaves out; a flag is present only
// when set.
export interface SearchParams {
  q?: string;
  in?: string;
  from?: string;
  after?: string;
  before?: string;
  unread?: true;
  starred?: true;
  attachment?: true;
}

// The value of `in` that keeps Junk and Trash in the search.
export const EVERY_MAILBOX = "all";

// The roles a search leaves out unless it names one of them.
const SET_ASIDE: ReadonlySet<string> = new Set(["junk", "trash"]);

const FLAGGED = "$flagged";

const textSchema = z.string().check(z.trim(), z.minLength(1));
const daySchema = z.iso.date();
// A flag as the route writes it.
const FLAG = "1";
const flagSchema = z.literal(FLAG);

function read<Value>(schema: z.ZodMiniType<Value>, value: unknown): Value | undefined {
  const parsed = schema.safeParse(value);
  return parsed.success ? parsed.data : undefined;
}

type Fields = ReadonlyMap<string, unknown>;

function isSet(value: unknown): boolean {
  return read(flagSchema, value) !== undefined;
}

function textsOf(fields: Fields): Omit<SearchParams, "unread" | "starred" | "attachment"> {
  const q = read(textSchema, fields.get("q"));
  const scope = read(textSchema, fields.get("in"));
  const from = read(textSchema, fields.get("from"));
  const after = read(daySchema, fields.get("after"));
  const before = read(daySchema, fields.get("before"));
  return {
    ...(q === undefined ? {} : { q }),
    ...(scope === undefined ? {} : { in: scope }),
    ...(from === undefined ? {} : { from }),
    ...(after === undefined ? {} : { after }),
    ...(before === undefined ? {} : { before }),
  };
}

function flagsOf(fields: Fields): Pick<SearchParams, "unread" | "starred" | "attachment"> {
  return {
    ...(isSet(fields.get("unread")) ? { unread: true } : {}),
    ...(isSet(fields.get("starred")) ? { starred: true } : {}),
    ...(isSet(fields.get("attachment")) ? { attachment: true } : {}),
  };
}

// The params of a route or a key, each one checked on its own: a param
// that fails reads as absent and anything else is left out.
export function readSearchParams(raw: unknown): SearchParams {
  const fields = new Map<string, unknown>(
    typeof raw === "object" && raw !== null ? Object.entries(raw) : [],
  );
  return { ...textsOf(fields), ...flagsOf(fields) };
}

function flagWord(set: true | undefined): string | undefined {
  return set === true ? FLAG : undefined;
}

// The key a search is held under: the params in the route's form, in a
// fixed order, so one search has one key.
export function searchKeyOf(params: SearchParams): string {
  const pairs: [string, string | undefined][] = [
    ["q", params.q],
    ["in", params.in],
    ["from", params.from],
    ["after", params.after],
    ["before", params.before],
    ["unread", flagWord(params.unread)],
    ["starred", flagWord(params.starred)],
    ["attachment", flagWord(params.attachment)],
  ];
  return JSON.stringify(
    Object.fromEntries(pairs.filter((pair): pair is [string, string] => pair[1] !== undefined)),
  );
}

// The params a key holds; a key that is no JSON holds none.
export function searchParamsOf(key: string): SearchParams {
  try {
    return readSearchParams(JSON.parse(key));
  } catch (error) {
    if (error instanceof SyntaxError) {
      return {};
    }
    throw error;
  }
}

// The scope of a search. An `in` that names no mailbox of the account
// reads as absent, so a stale link never widens a search to Junk and
// Trash.
export function scopeOf(params: SearchParams, mailboxes: readonly Mailbox[]): Scope {
  if (params.in === EVERY_MAILBOX) {
    return { kind: "all" };
  }
  const named = mailboxes.find((row) => row.id === params.in);
  if (named !== undefined) {
    return { kind: "one", mailboxId: named.id };
  }
  const aside = mailboxes.filter((row) => row.role !== null && SET_ASIDE.has(row.role));
  return { kind: "except", mailboxIds: aside.map((row) => row.id) };
}

function scopeFilter(scope: Scope): EmailFilter {
  switch (scope.kind) {
    case "one":
      return { inMailbox: scope.mailboxId };
    case "except":
      return { inMailboxOtherThan: scope.mailboxIds };
    default:
      return {};
  }
}

const DAY = /^(\d{4})-(\d{2})-(\d{2})$/u;

// The start of a calendar day in the device's zone as a UTCDate (RFC
// 8620 section 1.4).
function dayStart(day: string): string {
  const [, year, month, date] = DAY.exec(day) ?? [];
  const start = new Date(0);
  start.setFullYear(Number(year), Number(month) - 1, Number(date));
  start.setHours(0, 0, 0, 0);
  return start.toISOString().replace(/\.000Z$/u, "Z");
}

// The one FilterCondition a search sends (RFC 8621 section 4.4.1): the
// scope, the text, the sender, the range from the start of the `after`
// day to the start of the `before` day, the attachment and the two flags.
export function searchFilterOf(params: SearchParams, mailboxes: readonly Mailbox[]): EmailFilter {
  return {
    ...scopeFilter(scopeOf(params, mailboxes)),
    ...(params.q === undefined ? {} : { text: params.q }),
    ...(params.from === undefined ? {} : { from: params.from }),
    ...(params.after === undefined ? {} : { after: dayStart(params.after) }),
    ...(params.before === undefined ? {} : { before: dayStart(params.before) }),
    ...(params.attachment === true ? { hasAttachment: true } : {}),
    ...(params.unread === true ? { notKeyword: SEEN } : {}),
    ...(params.starred === true ? { hasKeyword: FLAGGED } : {}),
  };
}
