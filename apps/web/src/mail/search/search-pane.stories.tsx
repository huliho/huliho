// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Autocomplete } from "@base-ui/react/autocomplete";
import type { SearchParams, Sender } from "@huliho/core";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { useId, useRef } from "react";
import type { JSX } from "react";

import { Button } from "../../design-system/button";
import { EmptyState } from "../../design-system/empty-state";
import { ErrorState } from "../../design-system/error-state";
import { FIXED_NOW, MAILBOXES, SENDERS } from "../fixtures";
import { LIST_WIDTH_DEFAULT_PX } from "../list-width";
import { routed } from "../story-router";
import { LoadingRows } from "../thread-list";
import { SearchState, Slot } from "./search-body";
import type { BodyView, SlotView } from "./search-body";
import { tokensOf } from "./search-draft";
import { SearchHeader, Shortcuts } from "./search-field";
import { SearchList } from "./search-list";
import { suggestionsOf } from "./search-suggestions";
import type { Recent, SuggestionGroup } from "./search-suggestions";
import { emptyWords, resultsWords, scopeWords } from "./search-words";
import styles from "./search-pane.module.css";

const PARTS = { Button, EmptyState, ErrorState };
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
    <div className={styles.body}>
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
          <Shortcuts locale="en" onInsert={nothing} />
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

// A search with its tokens and the count in the slot.
export const Results = story({
  text: "offerte",
  params: RAN,
  slot: {
    kind: "results",
    sentence: resultsWords(RAN, 3, scope, "en"),
    count: { text: "3", at: 0 },
    widen: emptyWords(RAN, scope, "en").widen,
  },
  body: { kind: "nothing" },
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
