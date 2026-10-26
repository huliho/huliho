// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { queryOf, searchKeyOf, searchParamsOf } from "@huliho/core";
import type { MailCache, QueryContext, SearchParams } from "@huliho/core";
import { sendersQueryOptions } from "@huliho/state";
import { useQueries, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import type { FocusEvent, KeyboardEvent, RefObject } from "react";

import { recentSearches } from "./recent-searches";
import {
  draftOf,
  holdsNothing,
  paramsOf,
  settled,
  tokensOf,
  withOperator,
  withToken,
  withoutLast,
} from "./search-draft";
import type { Draft, Token } from "./search-draft";
import type { Shortcut } from "./search-field";
import { senderPrefix, suggestionsOf } from "./search-suggestions";
import type { Recent, Suggestion, SuggestionGroup } from "./search-suggestions";
import type { WordContext } from "./search-words";
import { usePressEnd } from "./use-press-end";
import type { RanSearch } from "./use-search";

// What the field reads beyond the search: where the senders come from,
// the recents' home and the day the presets count back from.
export interface FieldContext extends QueryContext {
  locale: WordContext["locale"];
  cache: MailCache;
  accountId: string;
  strict: boolean;
  today: number;
}

// The draft the route's search shows, followed while the user edits it;
// another search in the route starts it over.
function useDraft(ran: SearchParams, key: string | null, context: QueryContext) {
  const [held, setHeld] = useState(() => ({ key, draft: draftOf(ran, context) }));
  const current = held.key === key ? held : { key, draft: draftOf(ran, context) };
  if (current !== held) {
    setHeld(current);
  }
  const set = (draft: Draft): void => {
    setHeld({ key, draft });
  };
  return [current.draft, set] as const;
}

// The names the held mail gives the addresses of the sender tokens.
function useNames(context: FieldContext, tokens: readonly Token[]): Map<string, string> {
  const addresses = tokens.flatMap((token) => (token.key === "from" ? [token.value] : []));
  const answers = useQueries({
    queries: addresses.map((address) =>
      sendersQueryOptions(context.cache, context.accountId, address),
    ),
  });
  const names = new Map<string, string>();
  for (const sender of answers.flatMap((answer) => answer.data ?? [])) {
    if (sender.name !== null) {
      names.set(sender.email.toLowerCase(), sender.name);
    }
  }
  return names;
}

function recentsOf(context: FieldContext): Recent[] {
  return recentSearches(context.accountId, context.strict).flatMap((key) => {
    const params = searchParamsOf(key);
    return holdsNothing(params) ? [] : [{ key, params, text: queryOf(params, context) }];
  });
}

// The field: its draft, the suggestions it calls up, whether the list
// stands in the body and what its keys, picks and shortcuts do.
export interface Field {
  draft: Draft;
  tokens: Token[];
  words: WordContext;
  groups: SuggestionGroup[];
  listShown: boolean;
  focused: boolean;
  setDraft: (draft: Draft) => void;
  setFocused: (focused: boolean) => void;
  setHighlighted: (suggestion: Suggestion | undefined) => void;
  // The value of the row Up and Down walked to, if any.
  highlighted: string | undefined;
  run: (params: SearchParams) => void;
}

export function useField(context: FieldContext, ran: SearchParams, search: RanSearch): Field {
  const [draft, setDraft] = useDraft(ran, search.key, context);
  const [focused, setFocused] = useState(false);
  // The rows are built anew on every render, so the walked row is held by
  // its value; holding the row itself would set new state on each one.
  const [highlighted, setHighlighted] = useState<string | undefined>(undefined);
  const prefix = senderPrefix(draft.text);
  const senders = useQuery({
    ...sendersQueryOptions(context.cache, context.accountId, prefix ?? ""),
    enabled: prefix !== null,
  });
  const tokens = tokensOf(draft.operators);
  const today = new Date(context.today);
  const words = { ...context, names: useNames(context, tokens), today };
  const groups = suggestionsOf({
    ...context,
    text: draft.text,
    senders: prefix === null ? [] : (senders.data ?? []),
    recents: recentsOf(context),
    today,
  });
  const edited = searchKeyOf(paramsOf(draft, context)) !== searchKeyOf(ran);
  return {
    draft,
    tokens,
    words,
    groups,
    listShown: search.key === null || (focused && edited),
    focused,
    setDraft,
    setFocused,
    setHighlighted: (suggestion) => {
      setHighlighted(suggestion?.value);
    },
    highlighted,
    run: (params) => {
      setDraft(draftOf(params, context));
      search.run(params);
    },
  };
}

// What the field's keys, picks and shortcuts do.
export interface FieldActions {
  onText: (text: string) => void;
  onKeyDown: (event: KeyboardEvent<HTMLInputElement>) => void;
  onPick: (suggestion: Suggestion) => void;
  onInsert: (shortcut: Shortcut) => void;
  onRemove: (index: number) => void;
  onFocus: (event: FocusEvent) => void;
  onBlur: (event: FocusEvent) => void;
}

function atStart(input: HTMLInputElement): boolean {
  return input.selectionStart === 0 && input.selectionEnd === 0;
}

// Enter runs the draft where no row is walked to; Escape clears a field
// that holds something and closes an empty one; Backspace at the start
// takes the last token.
function keysOf(field: Field, context: QueryContext, onClose: () => void) {
  return (event: KeyboardEvent<HTMLInputElement>): void => {
    const { draft } = field;
    if (event.key === "Enter" && field.highlighted === undefined) {
      event.preventDefault();
      field.run(paramsOf(draft, context));
    } else if (event.key === "Escape") {
      event.preventDefault();
      if (holdsNothing(paramsOf(draft, context))) {
        onClose();
      } else {
        field.setDraft({ operators: {}, text: "" });
      }
    } else if (
      event.key === "Backspace" &&
      field.tokens.length > 0 &&
      atStart(event.currentTarget)
    ) {
      event.preventDefault();
      field.setDraft(withoutLast(draft));
    }
  };
}

// The field's input and the row of shortcuts under it, which count as the
// field having the focus.
export interface FieldRefs {
  input: RefObject<HTMLInputElement | null>;
  shortcuts: RefObject<HTMLDivElement | null>;
}

function isFieldPart(refs: FieldRefs, target: EventTarget | null): boolean {
  return (
    target instanceof Node &&
    (target === refs.input.current || refs.shortcuts.current?.contains(target) === true)
  );
}

export function useFieldActions(
  field: Field,
  context: QueryContext,
  refs: FieldRefs,
  onClose: () => void,
): FieldActions {
  const { draft } = field;
  const afterPress = usePressEnd();
  return {
    onText: (text) => {
      field.setDraft(settled({ ...draft, text }, context));
    },
    onKeyDown: keysOf(field, context, onClose),
    onPick: (suggestion) => {
      if (suggestion.kind === "recent") {
        field.run(suggestion.params);
      } else {
        const next = suggestion.kind === "token" ? withToken(draft, suggestion.token) : draft;
        field.run(paramsOf(next, context));
      }
    },
    onInsert: (shortcut) => {
      if (shortcut === "from" || shortcut === "in") {
        const spaced = draft.text === "" || /\s$/u.test(draft.text) ? draft.text : `${draft.text} `;
        field.setDraft({ ...draft, text: `${spaced}${shortcut}:` });
      } else {
        field.setDraft(withOperator(draft, { key: shortcut }));
      }
      refs.input.current?.focus();
    },
    onRemove: (index) => {
      const kept = field.tokens.filter((_token, at) => at !== index);
      const cleared: Draft = { ...draft, operators: {} };
      field.setDraft(kept.reduce((next, token) => withOperator(next, token), cleared));
      refs.input.current?.focus();
    },
    onFocus: (event) => {
      field.setFocused(isFieldPart(refs, event.target));
    },
    onBlur: (event) => {
      afterPress(event.relatedTarget, (target) => {
        field.setFocused(isFieldPart(refs, target));
      });
    },
  };
}
