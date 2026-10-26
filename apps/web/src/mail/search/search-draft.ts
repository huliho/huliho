// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { parseQuery, queryOf, senderTerms } from "@huliho/core";
import type { QueryContext, SearchParams } from "@huliho/core";

// One completed operator, as a token in the field shows it.
export type Token =
  | { key: "from" | "in" | "after" | "before"; value: string }
  | { key: "unread" | "starred" | "attachment" };

// What the field holds: the operators already tokens and the text after
// them, which runs as typed.
export interface Draft {
  operators: SearchParams;
  text: string;
}

function valued(key: "in" | "after" | "before", value: string | undefined): Token[] {
  return value === undefined ? [] : [{ key, value }];
}

function flag(key: "unread" | "starred" | "attachment", set: true | undefined): Token[] {
  return set === true ? [{ key }] : [];
}

// The tokens in the order the field draws them, the order `queryOf`
// writes the operators in.
export function tokensOf(params: SearchParams): Token[] {
  return [
    ...senderTerms(params.from ?? "").map((value): Token => ({ key: "from", value })),
    ...valued("in", params.in),
    ...valued("after", params.after),
    ...valued("before", params.before),
    ...flag("unread", params.unread),
    ...flag("starred", params.starred),
    ...flag("attachment", params.attachment),
  ];
}

function operatorOf(token: Token): SearchParams {
  switch (token.key) {
    case "from":
      return { from: token.value };
    case "in":
      return { in: token.value };
    case "after":
      return { after: token.value };
    case "before":
      return { before: token.value };
    case "unread":
      return { unread: true };
    case "starred":
      return { starred: true };
    default:
      return { attachment: true };
  }
}

// Two sets of params as one: the senders add up, the rest of `more` wins.
function merged(base: SearchParams, more: SearchParams): SearchParams {
  const from = [base.from, more.from].filter((part) => part !== undefined).join(" ");
  return { ...base, ...more, ...(from === "" ? {} : { from }) };
}

function fromTokens(tokens: readonly Token[]): SearchParams {
  return tokens.reduce<SearchParams>((params, token) => merged(params, operatorOf(token)), {});
}

// The params a draft runs: its tokens, then what its text adds.
export function paramsOf(draft: Draft, context: QueryContext): SearchParams {
  return merged(draft.operators, parseQuery(draft.text, context));
}

// The draft a search shows: its operators as tokens and its text, which
// `queryOf` quotes where an operator would claim a piece of it.
export function draftOf(params: SearchParams, context: QueryContext): Draft {
  return {
    operators: fromTokens(tokensOf(params)),
    text: params.q === undefined ? "" : queryOf({ q: params.q }, context),
  };
}

// A run of white space or a quoted run, the pieces that end a word.
const BREAKS = /\s+|"[^"]*"?/gu;

// Where the word being typed starts: after the last white space outside
// a quoted run.
function tailStart(text: string): number {
  let start = 0;
  for (const match of text.matchAll(BREAKS)) {
    if (!match[0].startsWith('"')) {
      start = match.index + match[0].length;
    }
  }
  return start;
}

// The word being typed, which the suggestions answer.
export function tailOf(text: string): string {
  return text.slice(tailStart(text));
}

function headOf(text: string): string {
  return text.slice(0, tailStart(text));
}

// The draft as the user types on: an operator the text completed, with
// white space after it, becomes a token; the word being typed stays.
export function settled(draft: Draft, context: QueryContext): Draft {
  const head = parseQuery(headOf(draft.text), context);
  const tokens = tokensOf(head);
  if (tokens.length === 0) {
    return draft;
  }
  const words = head.q === undefined ? "" : `${queryOf({ q: head.q }, context)} `;
  return {
    operators: merged(draft.operators, fromTokens(tokens)),
    text: `${words}${tailOf(draft.text)}`,
  };
}

// The draft with a token in place of the word being typed.
export function withToken(draft: Draft, token: Token): Draft {
  return {
    operators: merged(draft.operators, operatorOf(token)),
    text: headOf(draft.text).trimEnd(),
  };
}

// The draft with a token added, the text left as typed.
export function withOperator(draft: Draft, token: Token): Draft {
  return { ...draft, operators: merged(draft.operators, operatorOf(token)) };
}

// The draft without its last token, as Backspace at the field's start
// takes it.
export function withoutLast(draft: Draft): Draft {
  return { ...draft, operators: fromTokens(tokensOf(draft.operators).slice(0, -1)) };
}

// Whether a draft or a search holds nothing to run.
export function holdsNothing(params: SearchParams): boolean {
  return Object.keys(params).length === 0;
}
