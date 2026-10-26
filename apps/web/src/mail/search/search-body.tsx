// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ReactNode } from "react";

import type { Button } from "../../design-system/button";
import type { EmptyState } from "../../design-system/empty-state";
import type { ErrorState } from "../../design-system/error-state";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import type { EmptyWords } from "./search-words";
import styles from "./search-pane.module.css";

// The design system's button and states, which the shell hands in.
export interface SearchParts {
  Button: typeof Button;
  EmptyState: typeof EmptyState;
  ErrorState: typeof ErrorState;
}

// The count as the locale writes it and where it starts in the sentence.
export interface SlotCount {
  text: string;
  at: number;
}

// What the slot under the field says.
export type SlotView =
  | { kind: "none" | "searching" | "slow" | "canceled" }
  | { kind: "results"; sentence: string; count: SlotCount | null; widen: string | null };

interface SlotProps {
  locale: Locale;
  parts: SearchParts;
  view: SlotView;
  onCancel: () => void;
  onWiden: () => void;
}

// The sentence with its count in mono where the count stands in it.
function Counted({ sentence, count }: { sentence: string; count: SlotCount | null }) {
  if (count === null) {
    return sentence;
  }
  return (
    <>
      {sentence.slice(0, count.at)}
      <span className={styles.count}>{count.text}</span>
      {sentence.slice(count.at + count.text.length)}
    </>
  );
}

// The one place the search speaks: the count and the scope with the
// widening, the progress, the slow sentence with Cancel or the canceled
// line; nothing while the body carries the sentence.
export function Slot({ locale, parts, view, onCancel, onWiden }: SlotProps) {
  switch (view.kind) {
    case "none":
      return null;
    case "searching":
      return <p className={styles.slot}>{m.search_searching({}, { locale })}</p>;
    case "canceled":
      return <p className={styles.slot}>{m.search_canceled({}, { locale })}</p>;
    case "slow":
      return (
        <div className={styles.slot} data-tone="strong">
          <p className={styles.sentence}>{m.search_slow({}, { locale })}</p>
          <parts.Button onClick={onCancel}>{m.cancel_action({}, { locale })}</parts.Button>
        </div>
      );
    default:
      return (
        <p className={styles.slot}>
          <span className={styles.sentence}>
            <Counted sentence={view.sentence} count={view.count} />
            {view.widen !== null && (
              <>
                <span aria-hidden="true"> · </span>
                <button type="button" className={styles.widen} onClick={onWiden}>
                  {view.widen}
                </button>
              </>
            )}
          </span>
        </p>
      );
  }
}

// What the body shows once a search ran, in place of the suggestions.
export type BodyView =
  | { kind: "nothing" | "loading" | "offline" }
  | { kind: "failed"; deadline: boolean }
  | { kind: "empty"; words: EmptyWords };

interface StateProps {
  locale: Locale;
  parts: SearchParts;
  view: BodyView;
  // The list's still rows, which stand in for the results while they load.
  loading: ReactNode;
  onRetry: () => void;
  onWiden: () => void;
}

// The body's sentence for a search that ran: the still rows while it
// loads, the error with Try again, the empty word with the widening or
// the offline sentence.
export function SearchState({ locale, parts, view, loading, onRetry, onWiden }: StateProps) {
  switch (view.kind) {
    case "loading":
      return loading;
    case "offline":
      return <parts.EmptyState message={m.search_offline({}, { locale })} />;
    case "failed":
      return (
        <parts.ErrorState
          message={
            view.deadline ? m.search_deadline({}, { locale }) : m.search_failed({}, { locale })
          }
          retryLabel={m.retry_action({}, { locale })}
          onRetry={onRetry}
        />
      );
    case "empty":
      return (
        <div className={styles.empty}>
          <parts.EmptyState message={view.words.title} />
          <p className={styles.emptyHint}>{view.words.hint}</p>
          {view.words.widen !== null && (
            <parts.Button onClick={onWiden}>{view.words.widen}</parts.Button>
          )}
        </div>
      );
    default:
      return null;
  }
}
