// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { EVERY_MAILBOX, scopeOf } from "@huliho/core";
import type { Mailbox, SearchParams } from "@huliho/core";

import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import type { Token } from "./search-draft";

// What a token shows: its operator word in English whatever the locale,
// then its value as a person reads it.
export interface TokenWords {
  operator: string;
  value: string;
}

export interface WordContext {
  locale: Locale;
  mailboxes: readonly Mailbox[];
  // The names the held mail gives addresses, keyed in lower case.
  names: ReadonlyMap<string, string>;
  today: Date;
}

const OPERATORS: Record<Token["key"], string> = {
  from: "from:",
  in: "in:",
  after: "after:",
  before: "before:",
  unread: "is:",
  starred: "is:",
  attachment: "has:",
};

// The word `in:` takes for every mailbox, Junk and Trash included.
export const ANYWHERE = "anywhere";

const DAY = /^(\d{4})-(\d{2})-(\d{2})$/u;
const QUOTED = /^"([^"]*)"$/u;

// A calendar day as the locale writes it, the year only where it is not
// this one.
function dayWords(day: string, { locale, today }: WordContext): string {
  const [, year, month, date] = DAY.exec(day) ?? [];
  const at = new Date(Number(year), Number(month) - 1, Number(date));
  const thisYear = at.getFullYear() === today.getFullYear();
  return new Intl.DateTimeFormat(locale, {
    day: "numeric",
    month: "short",
    ...(thisYear ? {} : { year: "numeric" }),
  }).format(at);
}

// A sender by the name the held mail gives it, else as typed.
function senderWords(value: string, { names }: WordContext): string {
  return names.get(value.toLowerCase()) ?? QUOTED.exec(value)?.[1] ?? value;
}

function scopeWord(value: string, { mailboxes }: WordContext): string {
  if (value === EVERY_MAILBOX) {
    return ANYWHERE;
  }
  return mailboxes.find((row) => row.id === value)?.name ?? value;
}

function valueWords(token: Token, context: WordContext): string {
  switch (token.key) {
    case "from":
      return senderWords(token.value, context);
    case "in":
      return scopeWord(token.value, context);
    case "after":
    case "before":
      return dayWords(token.value, context);
    default:
      return token.key;
  }
}

export function tokenWords(token: Token, context: WordContext): TokenWords {
  return { operator: OPERATORS[token.key], value: valueWords(token, context) };
}

// The scope as the sentences select it: every mailbox but Junk and
// Trash, one mailbox by its name or all of them.
export interface ScopeWords {
  scope: "default" | "one" | "all";
  mailbox: string;
}

export function scopeWords(params: SearchParams, mailboxes: readonly Mailbox[]): ScopeWords {
  const scope = scopeOf(params, mailboxes);
  if (scope.kind === "one") {
    const name = mailboxes.find((row) => row.id === scope.mailboxId)?.name ?? "";
    return { scope: "one", mailbox: name };
  }
  return { scope: scope.kind === "all" ? "all" : "default", mailbox: "" };
}

// The results header: the count with the text and the scope, or the text
// and the scope alone where the server gives no total.
export function resultsWords(
  params: SearchParams,
  total: number | null,
  scope: ScopeWords,
  locale: Locale,
): string {
  const q = params.q;
  if (total === null) {
    return q === undefined
      ? m.search_results_open_filters(scope, { locale })
      : m.search_results_open_text({ ...scope, q }, { locale });
  }
  return q === undefined
    ? m.search_results_filters({ ...scope, count: total }, { locale })
    : m.search_results_text({ ...scope, count: total, q }, { locale });
}

// The empty state: what found nothing, how to look further and the
// widening the scope allows; none where Junk and Trash are in already.
export interface EmptyWords {
  title: string;
  hint: string;
  widen: string | null;
}

export function emptyWords(params: SearchParams, scope: ScopeWords, locale: Locale): EmptyWords {
  return {
    title:
      params.q === undefined
        ? m.search_empty_filters({}, { locale })
        : m.search_empty_text({ q: params.q }, { locale }),
    hint: m.search_empty_hint(scope, { locale }),
    widen: scope.scope === "all" ? null : m.search_widen(scope, { locale }),
  };
}

// The params the widening runs: every mailbox but Junk and Trash from
// one mailbox, Junk and Trash as well from there.
export function widened(params: SearchParams, scope: ScopeWords): SearchParams {
  const { in: _scope, ...rest } = params;
  return scope.scope === "one" ? rest : { ...rest, in: EVERY_MAILBOX };
}
