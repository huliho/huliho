// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { EVERY_MAILBOX, foldText, startsAWord, treeOrder } from "@huliho/core";
import type { Mailbox, SearchParams, Sender } from "@huliho/core";

import { tailOf } from "./search-draft";
import type { Token } from "./search-draft";

// One row under the field: the plain search, a token it fills in or a
// recent search. `value` names the row among the others.
export type Suggestion =
  | { kind: "run"; value: string; text: string }
  | { kind: "token"; value: string; token: Token; sender?: Sender; preset?: PresetSpan }
  | { kind: "recent"; value: string; key: string; params: SearchParams; text: string };

// The rows in their groups: the suggestions, the widening under its rule
// and the recents under their label.
export interface SuggestionGroup {
  value: "suggest" | "anywhere" | "recent";
  items: Suggestion[];
}

// A recent search with the text the field writes for it.
export interface Recent {
  key: string;
  params: SearchParams;
  text: string;
}

export interface SuggestionInput {
  text: string;
  // The senders the worker answered for the word being typed.
  senders: readonly Sender[];
  mailboxes: readonly Mailbox[];
  recents: readonly Recent[];
  today: Date;
  locale: string;
}

// The spans a date preset reaches back, in days or in months.
const PRESETS = [
  { span: "week", days: 7, months: 0 },
  { span: "month", days: 0, months: 1 },
  { span: "year", days: 0, months: 12 },
] as const;

export type PresetSpan = (typeof PRESETS)[number]["span"];

const OPERATOR = /^([a-z]+):(.*)$/iu;

// The calendar day a preset starts on, in the device's zone; a month back
// from the 31st lands on the last day of a shorter month.
export function presetDay(span: PresetSpan, today: Date): string {
  const preset = PRESETS.find((entry) => entry.span === span) ?? PRESETS[0];
  const month = new Date(today.getFullYear(), today.getMonth() - preset.months, 1);
  const last = new Date(month.getFullYear(), month.getMonth() + 1, 0).getDate();
  const day = new Date(
    month.getFullYear(),
    month.getMonth(),
    Math.min(today.getDate(), last) - preset.days,
  );
  const shown = String(day.getMonth() + 1).padStart(2, "0");
  return `${String(day.getFullYear())}-${shown}-${String(day.getDate()).padStart(2, "0")}`;
}

// What the worker is asked for: the senders after `from:` or those the
// word being typed starts; nothing for another operator or no word.
export function senderPrefix(text: string): string | null {
  const tail = tailOf(text);
  const operator = OPERATOR.exec(tail);
  if (operator === null) {
    return tail === "" ? null : tail;
  }
  return operator[1]?.toLowerCase() === "from" ? (operator[2] ?? "") : null;
}

function senderRows(senders: readonly Sender[]): Suggestion[] {
  return senders.map((sender) => ({
    kind: "token",
    value: `from:${sender.email}`,
    token: { key: "from", value: sender.email },
    sender,
  }));
}

function mailboxRows(input: SuggestionInput, prefix: string): Suggestion[] {
  const byId = new Map(input.mailboxes.map((row) => [row.id, row]));
  return treeOrder(input.mailboxes, input.locale).flatMap((id): Suggestion[] => {
    const mailbox = byId.get(id);
    return mailbox === undefined || !startsAWord(mailbox.name, prefix)
      ? []
      : [{ kind: "token", value: `in:${id}`, token: { key: "in", value: id } }];
  });
}

function presetRows(key: "after" | "before", today: Date): Suggestion[] {
  return PRESETS.map(({ span }) => ({
    kind: "token",
    value: `${key}:${span}`,
    token: { key, value: presetDay(span, today) },
    preset: span,
  }));
}

function flagRows(keys: readonly ("unread" | "starred" | "attachment")[], prefix: string) {
  return keys
    .filter((key) => key.startsWith(prefix.toLowerCase()))
    .map((key): Suggestion => ({ kind: "token", value: `flag:${key}`, token: { key } }));
}

function plainRows(input: SuggestionInput): SuggestionGroup[] {
  const text = input.text.trim();
  const tail = tailOf(input.text);
  const run: Suggestion[] = text === "" ? [] : [{ kind: "run", value: "run", text }];
  const more = tail === "" ? [] : [...senderRows(input.senders), ...mailboxRows(input, tail)];
  return [{ value: "suggest", items: [...run, ...more] }];
}

// The rows an operator being typed asks for, by its word; an operator the
// parser leaves as text gets the rows plain text gets.
function operatorRows(word: string, value: string, input: SuggestionInput): SuggestionGroup[] {
  switch (word) {
    case "from":
      return [{ value: "suggest", items: senderRows(input.senders) }];
    case "in": {
      const anywhere: Suggestion = {
        kind: "token",
        value: "in:anywhere",
        token: { key: "in", value: EVERY_MAILBOX },
      };
      return [
        { value: "suggest", items: mailboxRows(input, value) },
        { value: "anywhere", items: startsAWord("anywhere", value) ? [anywhere] : [] },
      ];
    }
    case "after":
    case "before":
      return [{ value: "suggest", items: presetRows(word, input.today) }];
    case "is":
      return [{ value: "suggest", items: flagRows(["unread", "starred"], value) }];
    case "has":
      return [{ value: "suggest", items: flagRows(["attachment"], value) }];
    default:
      return plainRows(input);
  }
}

function recentRows(input: SuggestionInput): SuggestionGroup {
  const wanted = foldText(input.text.trim());
  const items = input.recents
    .filter((recent) => foldText(recent.text).includes(wanted))
    .map(({ key, params, text }): Suggestion => ({
      kind: "recent",
      value: `recent:${key}`,
      key,
      params,
      text,
    }));
  return { value: "recent", items };
}

// The rows under the field for what it holds: after an operator what
// fills it in, else the plain search first, then the senders and the
// mailboxes the word being typed starts; then the recents that match.
export function suggestionsOf(input: SuggestionInput): SuggestionGroup[] {
  const operator = OPERATOR.exec(tailOf(input.text));
  const word = operator?.[1]?.toLowerCase();
  const groups =
    operator === null || word === undefined
      ? plainRows(input)
      : operatorRows(word, operator[2] ?? "", input);
  return [...groups, recentRows(input)].filter((group) => group.items.length > 0);
}
