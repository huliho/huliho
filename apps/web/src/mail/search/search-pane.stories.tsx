// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Autocomplete } from "@base-ui/react/autocomplete";
import { searchKeyOf } from "@huliho/core";
import type { SearchParams, Sender } from "@huliho/core";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { useId, useRef } from "react";
import type { JSX } from "react";

import { Button } from "../../design-system/button";
import { EmptyState } from "../../design-system/empty-state";
import { ErrorState } from "../../design-system/error-state";
import { fixtureCache } from "../fixture-cache";
import { FASTMAIL, FIXED_NOW, INBOX_PAGE, MAILBOXES, SENDERS } from "../fixtures";
import { LIST_WIDTH_DEFAULT_PX } from "../list-width";
import { startOfDay } from "../row-time";
import { routed } from "../story-router";
import { LoadingRows, ThreadList } from "../thread-list";
import { SearchState, Slot } from "./search-body";
import type { BodyView, SlotView } from "./search-body";
import { tokensOf } from "./search-draft";
import { SearchHeader, Shortcuts } from "./search-field";
import { SearchList } from "./search-list";
import { resultsSource } from "./search-results";
import { suggestionsOf } from "./search-suggestions";
import type { Recent, SuggestionGroup } from "./search-suggestions";
import { emptyWords, resultsWords, scopeWords } from "./search-words";
import styles from "./search-pane.module.css";

const PARTS = { Button, EmptyState, ErrorState, ThreadList };
const NAMES = new Map(
  SENDERS.flatMap((sender) => (sender.name === null ? [] : [[sender.email, sender.name]])),
);
const WORDS = { locale: "en" as const, mailboxes: MAILBOXES, names: NAMES, today: FIXED_NOW };
const RECENTS: Recent[] = [
  { key: "r1", params: { q: "offerte dakkapel" }, text: "offerte dakkapel" },
  { key: "r2", params: { q: "factuur", unread: true }, text: "is:unread factuur" },
  { key: "r3", params: { from: "sanne@fastmail.com", in: "mb-facturen" }, text: "" },
];
const NO_PARAMS: SearchParams = {};
const RAN: SearchParams = {
  q: "offerte",
  from: "pieter@blom-installaties.example",
  in: "mb-facturen",
};

function nothing(): void {
  // A drawn pane runs nothing.
}

// A search over every mailbox but Junk and Trash and the three threads it
// found, each acting from the mailbox it stands in.
const FOUND: SearchParams = { q: "offerte", from: "pieter@blom-installaties.example" };
const FOUND_IN = ["mb-inbox", "mb-archive", "mb-facturen"];
const FOUND_ROWS = FOUND_IN.flatMap((mailboxId, index) => {
  const row = INBOX_PAGE.rows.at(index);
  return row === undefined ? [] : [{ ...row, mailboxId }];
});
const FOUND_KEY = searchKeyOf(FOUND);
const FOUND_CACHE = fixtureCache({
  [`search:${FOUND_KEY}/0`]: { rows: FOUND_ROWS, total: FOUND_ROWS.length, pending: 0 },
});

function Found(): JSX.Element {
  const context = {
    cache: FOUND_CACHE,
    accountId: FASTMAIL.id,
    locale: "en",
    mailboxes: MAILBOXES,
    result: { actsFrom: null },
  };
  return (
    <ThreadList
      locale="en"
      today={startOfDay(FIXED_NOW)}
      cache={FOUND_CACHE}
      accountId={FASTMAIL.id}
      source={resultsSource(context, FOUND_KEY, FOUND)}
      mailboxes={MAILBOXES}
      online
      openThreadId={null}
      empty={null}
      onOpen={nothing}
      onClose={nothing}
    />
  );
}

interface Drawn {
  text?: string;
  params?: SearchParams;
  groups?: SuggestionGroup[];
  hint?: boolean;
  slot?: SlotView;
  body?: BodyView;
}

interface DrawnBodyProps {
  groups: SuggestionGroup[] | undefined;
  hint: boolean;
  hintId: string;
  text: string;
  body: BodyView | undefined;
}

// The body under the slot: the list where a story draws suggestions and
// the state where it draws a search that ran.
function DrawnBody({ groups, hint, hintId, text, body }: DrawnBodyProps): JSX.Element {
  return (
    <div className={styles.body} data-results={body?.kind === "results" || undefined}>
      {groups !== undefined && (
        <SearchList
          groups={groups}
          words={WORDS}
          hint={hint}
          hintId={hintId}
          typed={text}
          onPick={nothing}
        />
      )}
      {body !== undefined && (
        <SearchState
          locale="en"
          parts={PARTS}
          view={body}
          loading={<LoadingRows locale="en" />}
          results={<Found />}
          onRetry={nothing}
          onWiden={nothing}
        />
      )}
    </div>
  );
}

// The pane's parts as the pane composes them, drawn in one state each,
// so a story shows that state without a server or a clock.
function DrawnPane({
  text = "",
  params = NO_PARAMS,
  groups,
  hint = false,
  slot,
  body,
}: Drawn): JSX.Element {
  const input = useRef<HTMLInputElement>(null);
  const hintId = useId();
  const hintShown = groups !== undefined && hint;
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        blockSize: "70vh",
        maxInlineSize: `${String(LIST_WIDTH_DEFAULT_PX)}px`,
      }}
    >
      <div className={styles.pane}>
        <Autocomplete.Root
          inline={groups !== undefined}
          open={groups !== undefined}
          items={groups ?? []}
          filter={null}
          value={text}
        >
          <SearchHeader
            locale="en"
            tokens={tokensOf(params)}
            words={WORDS}
            inputRef={input}
            sentence=""
            hintId={hintId}
            hintShown={hintShown}
            onKeyDown={nothing}
            onRemove={nothing}
            onClose={nothing}
          />
          {body?.kind !== "results" && <Shortcuts locale="en" onInsert={nothing} />}
          {slot !== undefined && (
            <Slot locale="en" parts={PARTS} view={slot} onCancel={nothing} onWiden={nothing} />
          )}
          <DrawnBody groups={groups} hint={hint} hintId={hintId} text={text} body={body} />
        </Autocomplete.Root>
      </div>
    </div>
  );
}

function suggested(text: string, senders: readonly Sender[] = []): SuggestionGroup[] {
  return suggestionsOf({
    text,
    senders,
    mailboxes: MAILBOXES,
    recents: RECENTS,
    today: FIXED_NOW,
    locale: "en",
  });
}

const scope = scopeWords(RAN, MAILBOXES);
const foundScope = scopeWords(FOUND, MAILBOXES);

function story(drawn: Drawn): StoryObj {
  return { render: () => routed(() => <DrawnPane {...drawn} />) };
}

const meta: Meta = {
  title: "Mail/SearchPane",
};

export default meta;

// Opened: the hint over the recents, the shortcuts under the field.
export const Opened = story({ groups: suggested(""), hint: true });

// A word typed: the plain search, the senders it starts and the recents that match.
export const Suggestions = story({ text: "pie", groups: suggested("pie", SENDERS.slice(0, 3)) });

// `in:` typed: the tree's mailboxes and the widening under its rule.
export const Mailboxes = story({ text: "in:", groups: suggested("in:") });

// A search with its token, the count in the slot and the results under
// it, each naming the mailbox it acts from.
export const Results = story({
  text: "offerte",
  params: FOUND,
  slot: {
    kind: "results",
    sentence: resultsWords(FOUND, FOUND_ROWS.length, foundScope, "en"),
    count: { text: String(FOUND_ROWS.length), at: 0 },
    widen: emptyWords(FOUND, foundScope, "en").widen,
  },
  body: { kind: "results" },
});

export const Searching = story({
  text: "offerte",
  params: RAN,
  slot: { kind: "searching" },
  body: { kind: "loading" },
});

export const Slow = story({
  text: "offerte",
  params: RAN,
  slot: { kind: "slow" },
  body: { kind: "loading" },
});

export const Canceled = story({
  text: "offerte",
  params: RAN,
  slot: { kind: "canceled" },
  body: { kind: "nothing" },
});

export const Empty = story({
  text: "offerte",
  params: RAN,
  body: { kind: "empty", words: emptyWords(RAN, scope, "en") },
});

export const Deadline = story({
  text: "offerte",
  params: RAN,
  body: { kind: "failed", deadline: true },
});

export const Failed = story({
  text: "offerte",
  params: RAN,
  body: { kind: "failed", deadline: false },
});

export const Offline = story({ text: "offerte", params: RAN, body: { kind: "offline" } });
