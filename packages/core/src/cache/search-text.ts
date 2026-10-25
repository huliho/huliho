// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "../jmap/schemas";
import { z } from "../schema";
import { EVERY_MAILBOX } from "./search";
import type { SearchParams } from "./search";
import { treeOrder } from "./tree-order";

// What the field's text is read against: the account's mailboxes for
// `in:` and the locale a typed date is written in.
export interface QueryContext {
  mailboxes: readonly Mailbox[];
  locale: string;
}

// One piece of the text: an operator word with its value, a quoted run
// or a run of anything but white space.
const TOKEN = /([A-Za-z]+):("[^"]*"|\S*)|"[^"]*"|\S+/gu;
// A value in double quotes, which may hold white space.
const QUOTED = /^"([^"]*)"$/u;
// A value that needs no quotes.
const BARE = /^[^\s"]+$/u;
// A date as ISO 8601 writes it, or Gmail with slashes.
const YEAR_FIRST = /^(\d{4})[-/](\d{1,2})[-/](\d{1,2})$/u;
// A date as a locale writes it in digits: three numbers and two marks.
const NUMERIC = /^(\d{1,4})[-/.](\d{1,4})[-/.](\d{1,4})$/u;
// The word `in:` takes for every mailbox, Junk and Trash included.
const ANYWHERE = "anywhere";
// A day each part of which differs, to read a locale's order from.
const SAMPLE_DAY = new Date(2006, 10, 22);

const daySchema = z.iso.date();

// What one operator in the text sets.
type Found =
  | { key: "from"; value: string }
  | { key: "in" | "after" | "before"; value: string }
  | { key: "unread" | "starred" | "attachment" };

function unquoted(value: string): string {
  return QUOTED.exec(value)?.[1] ?? value;
}

// A calendar day from its parts, or null for one that does not exist.
function dayOf(year: string, month: string, day: string): string | null {
  const written = `${year}-${month.padStart(2, "0")}-${day.padStart(2, "0")}`;
  return daySchema.safeParse(written).success ? written : null;
}

// The order in which a locale writes the day, the month and the year.
function partsOrder(locale: string): string[] {
  const format = new Intl.DateTimeFormat(locale, {
    year: "numeric",
    month: "numeric",
    day: "numeric",
  });
  return format
    .formatToParts(SAMPLE_DAY)
    .map((part) => part.type)
    .filter((type) => type === "day" || type === "month" || type === "year");
}

// A typed date: year first as ISO 8601 or Gmail writes it, else in the
// locale's own order of digits.
function typedDay(value: string, locale: string): string | null {
  const [, year, month, day] = YEAR_FIRST.exec(value) ?? [];
  if (year !== undefined && month !== undefined && day !== undefined) {
    return dayOf(year, month, day);
  }
  const numbers = NUMERIC.exec(value)?.slice(1);
  if (numbers === undefined) {
    return null;
  }
  const parts = new Map(partsOrder(locale).map((type, index) => [type, numbers.at(index) ?? ""]));
  return dayOf(parts.get("year") ?? "", parts.get("month") ?? "", parts.get("day") ?? "");
}

// The mailbox a value names: by name in any case, the first in the
// tree's order where names repeat, else by id.
function mailboxNamed(value: string, { mailboxes, locale }: QueryContext): string | null {
  const wanted = value.toLowerCase();
  const named = new Set(
    mailboxes.filter((row) => row.name.toLowerCase() === wanted).map((row) => row.id),
  );
  const first = treeOrder(mailboxes, locale).find((id) => named.has(id));
  return first ?? mailboxes.find((row) => row.id === value)?.id ?? null;
}

function scopeIn(value: string, context: QueryContext): Found | null {
  const name = unquoted(value);
  if (name.toLowerCase() === ANYWHERE) {
    return { key: "in", value: EVERY_MAILBOX };
  }
  const id = mailboxNamed(name, context);
  return id === null ? null : { key: "in", value: id };
}

function dated(key: "after" | "before", value: string, locale: string): Found | null {
  const day = typedDay(unquoted(value), locale);
  return day === null ? null : { key, value: day };
}

// The operators that take a value, by their word.
const VALUED = new Map<string, (value: string, context: QueryContext) => Found | null>([
  ["from", (value) => (value === "" ? null : { key: "from", value })],
  ["in", scopeIn],
  ["after", (value, { locale }) => dated("after", value, locale)],
  ["before", (value, { locale }) => dated("before", value, locale)],
]);

// The operators that are whole as written, in lower case.
const WHOLE = new Map<string, Found>([
  ["is:unread", { key: "unread" }],
  ["is:starred", { key: "starred" }],
  ["has:attachment", { key: "attachment" }],
]);

// What an operator sets; null for one the text keeps as it stands: an
// unknown word, an empty value, a mailbox the tree lacks or a date that
// does not parse.
function operatorOf(word: string, value: string, context: QueryContext): Found | null {
  const name = word.toLowerCase();
  return WHOLE.get(`${name}:${value.toLowerCase()}`) ?? VALUED.get(name)?.(value, context) ?? null;
}

// The params the field's text holds: the operator words set theirs in
// any case and the rest is the text. A quoted run is never an operator.
export function parseQuery(text: string, context: QueryContext): SearchParams {
  const words: string[] = [];
  const senders: string[] = [];
  const params: SearchParams = {};
  for (const [whole, word, value] of text.matchAll(TOKEN)) {
    const found =
      word === undefined || value === undefined ? null : operatorOf(word, value, context);
    if (found === null) {
      words.push(whole);
    } else if (found.key === "from") {
      senders.push(found.value);
    } else if ("value" in found) {
      params[found.key] = found.value;
    } else {
      params[found.key] = true;
    }
  }
  return {
    ...params,
    ...(senders.length === 0 ? {} : { from: senders.join(" ") }),
    ...(words.length === 0 ? {} : { q: words.join(" ") }),
  };
}

// How `in:` names a mailbox in the text: by its name where that reads
// back as this mailbox, else by its id.
function scopeWord(scope: string, { mailboxes, locale }: QueryContext): string {
  if (scope === EVERY_MAILBOX) {
    return ANYWHERE;
  }
  const name = mailboxes.find((row) => row.id === scope)?.name ?? "";
  const written = BARE.test(name) ? name : `"${name}"`;
  const plain = name !== "" && !name.includes('"') && name.toLowerCase() !== ANYWHERE;
  return plain && mailboxNamed(name, { mailboxes, locale }) === scope ? written : scope;
}

// The text as `parseQuery` reads it back: a piece an operator would
// claim goes in quotes, with its own quotes dropped.
function textOf(q: string, context: QueryContext): string {
  return [...q.matchAll(TOKEN)]
    .map(([whole, word, value]) =>
      word !== undefined && value !== undefined && operatorOf(word, value, context) !== null
        ? `"${whole.replaceAll('"', "")}"`
        : whole,
    )
    .join(" ");
}

// The field's text for the params, which `parseQuery` reads back: the
// operators in a fixed order, then the text with each piece an operator
// would claim as a quoted run.
export function queryOf(params: SearchParams, context: QueryContext): string {
  const senders = [...(params.from ?? "").matchAll(TOKEN)].map(([term]) => `from:${term}`);
  return [
    ...senders,
    ...(params.in === undefined ? [] : [`in:${scopeWord(params.in, context)}`]),
    ...(params.after === undefined ? [] : [`after:${params.after}`]),
    ...(params.before === undefined ? [] : [`before:${params.before}`]),
    ...(params.unread === true ? ["is:unread"] : []),
    ...(params.starred === true ? ["is:starred"] : []),
    ...(params.attachment === true ? ["has:attachment"] : []),
    ...(params.q === undefined ? [] : [textOf(params.q, context)]),
  ].join(" ");
}
