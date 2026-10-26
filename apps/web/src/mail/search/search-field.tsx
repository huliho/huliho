// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Autocomplete } from "@base-ui/react/autocomplete";
import { Search, X } from "lucide-react";
import { useRef } from "react";
import type { KeyboardEvent, MouseEvent, RefObject } from "react";

import { ESCAPE } from "../../commands/keys";
import { cx } from "../../design-system/cx";
import iconButton from "../../design-system/icon-button.module.css";
import spoken from "../../design-system/spoken.module.css";
import { Tooltip } from "../../design-system/tooltip";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { useStatusText } from "../status-text";
import type { Token } from "./search-draft";
import { tokenWords } from "./search-words";
import type { WordContext } from "./search-words";
import styles from "./search-pane.module.css";

// A press on a token or a shortcut leaves the focus in the field, so a
// phone's keyboard stays up.
function keepFocus(event: MouseEvent): void {
  event.preventDefault();
}

interface TokenProps {
  token: Token;
  words: WordContext;
  onRemove: () => void;
}

// A completed operator, named by its operator and value; a press takes
// it out of the field.
function TokenButton({ token, words, onRemove }: TokenProps) {
  const { operator, value } = tokenWords(token, words);
  return (
    <button
      type="button"
      tabIndex={-1}
      className={styles.token}
      aria-label={m.search_token_remove(
        { token: `${operator} ${value}` },
        { locale: words.locale },
      )}
      onMouseDown={keepFocus}
      onClick={onRemove}
    >
      <span dir="ltr" className={styles.operator}>
        {operator}
      </span>
      <bdi>{value}</bdi>
    </button>
  );
}

interface CloseProps {
  locale: Locale;
  onClose: () => void;
}

// Close at the end of the header, with Escape in its tooltip.
function CloseButton({ locale, onClose }: CloseProps) {
  const close = m.search_close({}, { locale });
  return (
    <Tooltip label={close} keys={[ESCAPE]} align="end">
      <button
        type="button"
        className={cx(iconButton.button, styles.close)}
        aria-label={close}
        onClick={onClose}
      >
        <X className={iconButton.icon} aria-hidden="true" />
      </button>
    </Tooltip>
  );
}

export interface SearchHeaderProps {
  locale: Locale;
  tokens: readonly Token[];
  words: WordContext;
  inputRef: RefObject<HTMLInputElement | null>;
  // What the header's status region says now.
  sentence: string;
  // The id of the hint that describes the field. While the list shows
  // the hint under that id, the header holds no spoken copy of it.
  hintId: string;
  hintShown: boolean;
  onKeyDown: (event: KeyboardEvent<HTMLInputElement>) => void;
  onRemove: (index: number) => void;
  onClose: () => void;
}

// The pane header in search: the field in the name's slot, its tokens
// before the text, Close at the end and the header's status region.
export function SearchHeader({
  locale,
  tokens,
  words,
  inputRef,
  sentence,
  hintId,
  hintShown,
  onKeyDown,
  onRemove,
  onClose,
}: SearchHeaderProps) {
  const regionRef = useRef<HTMLSpanElement>(null);
  useStatusText(regionRef, sentence);
  const label = m.command_search({}, { locale });
  return (
    <div className={styles.header} role="search">
      <div className={styles.field}>
        <Search className={styles.icon} aria-hidden="true" />
        {tokens.map((token, index) => (
          <TokenButton
            key={`${String(index)}:${token.key}`}
            token={token}
            words={words}
            onRemove={() => {
              onRemove(index);
            }}
          />
        ))}
        <Autocomplete.Input
          ref={inputRef}
          dir="auto"
          className={styles.input}
          placeholder={label}
          aria-label={label}
          aria-describedby={hintId}
          enterKeyHint="search"
          spellCheck={false}
          onKeyDown={onKeyDown}
        />
      </div>
      {!hintShown && (
        <span id={hintId} className={spoken.spoken}>
          {m.search_hint({}, { locale })}
        </span>
      )}
      <CloseButton locale={locale} onClose={onClose} />
      <span ref={regionRef} role="status" className={spoken.spoken} />
    </div>
  );
}

// The shortcuts in the order they stand: two that start an operator and
// two that set one.
const SHORTCUTS = [
  { operator: "from", text: "from:" },
  { operator: "in", text: "in:" },
  { operator: "unread", text: "is:unread" },
  { operator: "attachment", text: "has:attachment" },
] as const;

export type Shortcut = (typeof SHORTCUTS)[number]["operator"];

interface ShortcutsProps {
  locale: Locale;
  onInsert: (shortcut: Shortcut) => void;
}

// The operator shortcuts under the field while it has the focus. Their
// words are English in every locale, so they read left to right.
export function Shortcuts({ locale, onInsert }: ShortcutsProps) {
  return (
    <div className={styles.shortcuts}>
      {SHORTCUTS.map(({ operator, text }) => (
        <button
          key={operator}
          type="button"
          dir="ltr"
          className={styles.shortcut}
          aria-label={m.search_shortcut({ operator }, { locale })}
          onMouseDown={keepFocus}
          onClick={() => {
            onInsert(operator);
          }}
        >
          {text}
        </button>
      ))}
    </div>
  );
}
