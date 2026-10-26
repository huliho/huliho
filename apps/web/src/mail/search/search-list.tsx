// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Autocomplete } from "@base-ui/react/autocomplete";

import { chordText } from "../../commands/keys";
import { Kbd } from "../../design-system/kbd";
import { m } from "../../paraglide/messages.js";
import { tokensOf } from "./search-draft";
import type { Suggestion, SuggestionGroup } from "./search-suggestions";
import { ANYWHERE, tokenWords } from "./search-words";
import type { TokenWords, WordContext } from "./search-words";
import styles from "./search-pane.module.css";

const ENTER = chordText({ key: "Enter" });

type TokenSuggestion = Extract<Suggestion, { kind: "token" }>;
type RecentSuggestion = Extract<Suggestion, { kind: "recent" }>;

// What a token row names after its operator: a preset by its span, the
// widening by its words, a sender by the name beside the address.
function tokenLabel(item: TokenSuggestion, words: WordContext) {
  const { locale } = words;
  const { operator, value } = tokenWords(item.token, words);
  const name = item.sender?.name ?? null;
  const label =
    item.preset === undefined
      ? value === ANYWHERE
        ? m.search_anywhere({}, { locale })
        : (name ?? value)
      : m.search_preset({ span: item.preset }, { locale });
  return { operator, label, detail: name === null ? null : (item.sender?.email ?? null) };
}

// The leading spaces keep the parts apart in the row's text, which the
// row's spoken name has to hold.
function TokenRow({ item, words }: { item: TokenSuggestion; words: WordContext }) {
  const { operator, label, detail } = tokenLabel(item, words);
  return (
    <>
      <span dir="ltr" className={styles.operator}>
        {operator}
      </span>
      <bdi className={styles.label}> {label}</bdi>
      {detail !== null && <bdi className={styles.detail}> {detail}</bdi>}
    </>
  );
}

function recentTokens(item: RecentSuggestion, words: WordContext): TokenWords[] {
  return tokensOf(item.params).map((token) => tokenWords(token, words));
}

function spokenToken({ operator, value }: TokenWords): string {
  return `${operator} ${value}`;
}

// The name a row of parts is read by; the plain search reads its own text.
function spokenOf(item: Suggestion, words: WordContext): string | undefined {
  switch (item.kind) {
    case "run":
      return undefined;
    case "token": {
      const { operator, label, detail } = tokenLabel(item, words);
      return detail === null ? `${operator} ${label}` : `${operator} ${label}, ${detail}`;
    }
    default:
      return [...recentTokens(item, words).map(spokenToken), item.params.q ?? ""].join(" ").trim();
  }
}

// Each character lowercased on its own, kept as it is where its lowercase
// form is longer, so every index of the result is one of the text.
function loweredInPlace(text: string): string {
  return Array.from(text, (char) => {
    const lower = char.toLowerCase();
    return lower.length === char.length ? lower : char;
  }).join("");
}

// The part of a recent's text the field holds, marked as the move
// picker marks a match.
function marked(text: string, typed: string) {
  const wanted = loweredInPlace(typed.trim());
  const at = wanted === "" ? -1 : loweredInPlace(text).indexOf(wanted);
  if (at === -1) {
    return text;
  }
  return (
    <>
      {text.slice(0, at)}
      <span className={styles.hit}>{text.slice(at, at + wanted.length)}</span>
      {text.slice(at + wanted.length)}
    </>
  );
}

interface RecentProps {
  item: RecentSuggestion;
  typed: string;
  words: WordContext;
}

// A recent search: its operators as small tokens, then its text.
function RecentRow({ item, typed, words }: RecentProps) {
  return (
    <>
      {recentTokens(item, words).map((token) => (
        <span key={spokenToken(token)} className={styles.mini}>
          {" "}
          <span dir="ltr">{token.operator}</span> <bdi>{token.value}</bdi>
        </span>
      ))}
      <bdi className={styles.label}> {marked(item.params.q ?? "", typed)}</bdi>
    </>
  );
}

interface RowProps {
  item: Suggestion;
  words: WordContext;
  typed: string;
  onPick: (item: Suggestion) => void;
}

function Row({ item, words, typed, onPick }: RowProps) {
  return (
    <Autocomplete.Item
      value={item}
      className={styles.item}
      aria-label={spokenOf(item, words)}
      onClick={() => {
        onPick(item);
      }}
    >
      {item.kind === "run" && (
        <>
          <span className={styles.label}>
            {m.search_for({ q: item.text }, { locale: words.locale })}
          </span>
          <span className={styles.cap}>
            <Kbd spoken={false}>{ENTER}</Kbd>
          </span>
        </>
      )}
      {item.kind === "token" && <TokenRow item={item} words={words} />}
      {item.kind === "recent" && <RecentRow item={item} typed={typed} words={words} />}
    </Autocomplete.Item>
  );
}

interface ListProps {
  groups: readonly SuggestionGroup[];
  words: WordContext;
  // Whether the hint heads the list, as it does until the first run. The
  // hint then carries the id the field is described by.
  hint: boolean;
  hintId: string;
  // What the field holds, which a recent marks where it matches.
  typed: string;
  onPick: (suggestion: Suggestion) => void;
}

// The rows under the field, the hint heading them until the first run:
// the suggestions, the widening under its rule and the recents under
// their label. Up and Down walk them from the field.
export function SearchList({ groups, words, hint, hintId, typed, onPick }: ListProps) {
  return (
    <>
      {hint && (
        <p id={hintId} className={styles.hint}>
          {m.search_hint({}, { locale: words.locale })}
        </p>
      )}
      <Autocomplete.List className={styles.list}>
        {(group: SuggestionGroup) => (
          <Autocomplete.Group key={group.value} items={group.items}>
            {group.value === "anywhere" && <div className={styles.rule} />}
            {group.value === "recent" && (
              <Autocomplete.GroupLabel className={styles.groupLabel}>
                {m.search_recent({}, { locale: words.locale })}
              </Autocomplete.GroupLabel>
            )}
            <Autocomplete.Collection>
              {(item: Suggestion) => (
                <Row key={item.value} item={item} words={words} typed={typed} onPick={onPick} />
              )}
            </Autocomplete.Collection>
          </Autocomplete.Group>
        )}
      </Autocomplete.List>
    </>
  );
}
