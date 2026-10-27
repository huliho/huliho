// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { MethodFailure, SERVER_UNAVAILABLE } from "@huliho/core";
import type { Mailbox, SearchParams } from "@huliho/core";

import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import type { BodyView, SlotCount, SlotView } from "./search-body";
import { emptyWords, resultsWords, scopeWords } from "./search-words";
import type { ScopeWords } from "./search-words";
import type { RanSearch } from "./use-search";

// What the pane shows for the search the route holds: the slot, the
// body in place of the suggestions and the status region's sentence.
export interface SearchViews {
  slot: SlotView;
  body: BodyView;
  sentence: string;
}

// What the views read of the search: where it stands and its first page.
export type ViewedSearch = Pick<RanSearch, "key" | "canceled" | "phase"> & {
  page: Pick<RanSearch["page"], "data" | "error">;
};

interface ViewInput {
  search: ViewedSearch;
  ran: SearchParams;
  online: boolean;
  mailboxes: readonly Mailbox[];
  locale: Locale;
}

// A server that gave up on the search at its deadline.
function tookTooLong(error: unknown): boolean {
  return error instanceof MethodFailure && error.type === SERVER_UNAVAILABLE;
}

function bodyOf({ search, ran, online, mailboxes, locale }: ViewInput): BodyView {
  if (search.key === null || search.canceled) {
    return { kind: "nothing" };
  }
  if (!online) {
    return { kind: "offline" };
  }
  const { data, error } = search.page;
  if (data !== undefined) {
    return data.rows.length === 0
      ? { kind: "empty", words: emptyWords(ran, scopeWords(ran, mailboxes), locale) }
      : { kind: "results" };
  }
  return error === null ? { kind: "loading" } : { kind: "failed", deadline: tookTooLong(error) };
}

// Stand in for the query and the mailbox name while the count is looked
// up: private-use code points, which no sentence and no count holds.
const PRIVATE_USE = 0xe0_00;
export const STAND_IN = {
  q: String.fromCodePoint(PRIVATE_USE),
  mailbox: String.fromCodePoint(PRIVATE_USE + 1),
} as const;

// The texts the stand-ins hold the place of.
interface Filled {
  q: string;
  mailbox: string;
}

// The results sentence over the stand-ins, so the count is the only
// text in it that comes from the search.
function shapeOf(
  ran: SearchParams,
  total: number | null,
  scope: ScopeWords,
  locale: Locale,
): string {
  const params = ran.q === undefined ? ran : { ...ran, q: STAND_IN.q };
  return resultsWords(params, total, { ...scope, mailbox: STAND_IN.mailbox }, locale);
}

// How far `text` moves what follows it: its length less the stand-in's,
// once per stand-in before the count.
function shiftBy(before: string, standIn: string, text: string): number {
  return (before.split(standIn).length - 1) * (text.length - 1);
}

// Where `count` starts in the sentence the stand-ins of `shape` are
// filled with: its place in the shape, moved past the texts filled in
// before it. Null where the shape has no count.
export function countAt(shape: string, count: string, filled: Filled): number | null {
  const at = shape.indexOf(count);
  if (at === -1) {
    return null;
  }
  const before = shape.slice(0, at);
  return (
    at + shiftBy(before, STAND_IN.q, filled.q) + shiftBy(before, STAND_IN.mailbox, filled.mailbox)
  );
}

function countOf(
  total: number | null,
  shape: string,
  filled: Filled,
  locale: Locale,
): SlotCount | null {
  if (total === null) {
    return null;
  }
  const text = new Intl.NumberFormat(locale).format(total);
  const at = countAt(shape, text, filled);
  return at === null ? null : { text, at };
}

function slotOf({ search, ran, online, mailboxes, locale }: ViewInput): SlotView {
  if (search.key === null || !online) {
    return { kind: "none" };
  }
  if (search.canceled) {
    return { kind: "canceled" };
  }
  if (search.phase !== "quiet") {
    return { kind: search.phase };
  }
  const data = search.page.data;
  if (data === undefined || data.rows.length === 0) {
    return { kind: "none" };
  }
  const scope = scopeWords(ran, mailboxes);
  const words = emptyWords(ran, scope, locale);
  const filled = { q: ran.q ?? "", mailbox: scope.mailbox };
  return {
    kind: "results",
    sentence: resultsWords(ran, data.total, scope, locale),
    count: countOf(data.total, shapeOf(ran, data.total, scope, locale), filled, locale),
    widen: words.widen,
  };
}

// The sentence the status region carries; a failure speaks through its
// own alert, so the region stays quiet then.
function sentenceOf({ slot, body }: Omit<SearchViews, "sentence">, locale: Locale): string {
  switch (slot.kind) {
    case "searching":
      return m.search_searching({}, { locale });
    case "slow":
      return m.search_slow({}, { locale });
    case "canceled":
      return m.search_canceled({}, { locale });
    case "results":
      return slot.sentence;
    default:
      break;
  }
  if (body.kind === "empty") {
    return body.words.title;
  }
  return body.kind === "offline" ? m.search_offline({}, { locale }) : "";
}

export function searchViews(input: ViewInput): SearchViews {
  const slot = slotOf(input);
  const body = bodyOf(input);
  return { slot, body, sentence: sentenceOf({ slot, body }, input.locale) };
}
