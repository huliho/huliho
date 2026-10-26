// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Autocomplete } from "@base-ui/react/autocomplete";
import { readSearchParams } from "@huliho/core";
import type { MailCache, Mailbox, SearchParams } from "@huliho/core";
import { useId, useLayoutEffect, useRef } from "react";
import type { ReactNode, RefObject } from "react";

import { ESCAPE } from "../../commands/keys";
import { useCommand } from "../../commands/use-command";
import { focusHeir } from "../../design-system/button";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { SEARCH_KEYS } from "./open-search";
import type { SearchRoute } from "./route-params";
import { SearchState, Slot } from "./search-body";
import type { SearchParts } from "./search-body";
import { SearchHeader, Shortcuts } from "./search-field";
import { SearchList } from "./search-list";
import type { Suggestion } from "./search-suggestions";
import { searchViews } from "./search-views";
import type { SearchViews } from "./search-views";
import { scopeWords, widened } from "./search-words";
import { useField, useFieldActions } from "./use-field";
import type { Field, FieldActions } from "./use-field";
import { useRanSearch } from "./use-search";
import type { RanSearch } from "./use-search";
import styles from "./search-pane.module.css";

export interface SearchPaneProps {
  locale: Locale;
  cache: MailCache;
  accountId: string;
  route: SearchRoute;
  mailboxes: readonly Mailbox[];
  strict: boolean;
  online: boolean;
  // The start of today, which the date presets count back from.
  today: number;
  // The design system's button and states and the list's still rows,
  // which the shell hands in.
  parts: SearchParts;
  loading: ReactNode;
  onRoute: (route: SearchRoute) => void;
  onClose: () => void;
}

function valueOf(item: Suggestion): string {
  return item.value;
}

// The field takes the focus as the search opens; `/` puts it back there
// and Escape outside the field closes the search.
function useFieldFocus(
  locale: Locale,
  input: RefObject<HTMLInputElement | null>,
  onClose: () => void,
): () => void {
  const focus = (): void => {
    input.current?.focus();
  };
  useLayoutEffect(focus, [input]);
  useCommand({
    id: "search.open",
    label: m.command_search({}, { locale }),
    group: "app",
    keys: SEARCH_KEYS,
    run: focus,
  });
  useCommand({
    id: "search.close",
    label: m.command_search_close({}, { locale }),
    group: "navigate",
    keys: [ESCAPE],
    run: onClose,
  });
  return focus;
}

interface PartsProps {
  locale: Locale;
  field: Field;
  actions: FieldActions;
  views: SearchViews;
  // The id of the hint that describes the field, which the list's hint
  // carries while it stands and the header's spoken copy otherwise.
  hintId: string;
}

interface TopProps extends PartsProps {
  inputRef: RefObject<HTMLInputElement | null>;
  shortcutsRef: RefObject<HTMLDivElement | null>;
  hintShown: boolean;
  onClose: () => void;
}

// The header with the field and, while the field has the focus, the
// shortcuts under it.
function Top(props: TopProps) {
  const { locale, field, actions, views, hintId, inputRef, shortcutsRef, hintShown, onClose } =
    props;
  return (
    <div onFocus={actions.onFocus} onBlur={actions.onBlur}>
      <SearchHeader
        locale={locale}
        tokens={field.tokens}
        words={field.words}
        inputRef={inputRef}
        sentence={views.sentence}
        hintId={hintId}
        hintShown={hintShown}
        onKeyDown={actions.onKeyDown}
        onRemove={actions.onRemove}
        onClose={onClose}
      />
      <div ref={shortcutsRef}>
        {field.focused && <Shortcuts locale={locale} onInsert={actions.onInsert} />}
      </div>
    </div>
  );
}

interface BodyProps extends PartsProps {
  search: RanSearch;
  ran: SearchParams;
  parts: SearchParts;
  loading: ReactNode;
  onWiden: () => void;
}

// The body: the suggestions while the field calls them up, else what the
// search came to.
function Body(props: BodyProps) {
  const { locale, field, actions, views, hintId, search, ran, parts, loading, onWiden } = props;
  return (
    <div className={styles.body} {...focusHeir}>
      {field.listShown ? (
        <SearchList
          groups={field.groups}
          words={field.words}
          hint={search.key === null}
          hintId={hintId}
          typed={field.draft.text}
          onPick={actions.onPick}
        />
      ) : (
        <SearchState
          locale={locale}
          parts={parts}
          view={views.body}
          loading={loading}
          onRetry={() => {
            field.run(ran);
          }}
          onWiden={onWiden}
        />
      )}
    </div>
  );
}

// The search in the list pane: the field with its tokens in the header,
// the shortcuts while it has the focus, the slot where the search speaks
// and the body under them.
export function SearchPane(props: SearchPaneProps) {
  const { locale, mailboxes, online } = props;
  const input = useRef<HTMLInputElement>(null);
  const shortcuts = useRef<HTMLDivElement>(null);
  const ran = readSearchParams(props.route);
  const search = useRanSearch(props, ran);
  const field = useField(props, ran, search);
  const actions = useFieldActions(field, props, { input, shortcuts }, props.onClose);
  const focus = useFieldFocus(locale, input, props.onClose);
  const views = searchViews({ search, ran, online, mailboxes, locale });
  const onWiden = (): void => {
    field.run(widened(ran, scopeWords(ran, mailboxes)));
  };
  const hintId = useId();
  const hintShown = field.listShown && search.key === null;
  const shared = { locale, field, actions, views, hintId };
  return (
    <div className={styles.pane}>
      {/* An inline list reads as expanded whatever `open` says, so it is inline only while shown. */}
      <Autocomplete.Root
        inline={field.listShown}
        open={field.listShown}
        items={field.groups}
        filter={null}
        value={field.draft.text}
        onValueChange={actions.onText}
        onItemHighlighted={field.setHighlighted}
        itemToStringValue={valueOf}
        autoHighlight
        locale={locale}
      >
        <Top
          {...shared}
          inputRef={input}
          shortcutsRef={shortcuts}
          hintShown={hintShown}
          onClose={props.onClose}
        />
        <Slot
          locale={locale}
          parts={props.parts}
          view={views.slot}
          onCancel={() => {
            focus();
            search.cancel();
          }}
          onWiden={onWiden}
        />
        <Body
          {...shared}
          search={search}
          ran={ran}
          parts={props.parts}
          loading={props.loading}
          onWiden={onWiden}
        />
      </Autocomplete.Root>
    </div>
  );
}
