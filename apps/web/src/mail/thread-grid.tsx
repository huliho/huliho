// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow } from "@huliho/core";
import type { KeyboardEvent, MouseEvent, RefObject } from "react";

import type { Chord } from "../commands/keys";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import type { ListCursor } from "./list-cursor";
import type { ListMarker } from "./list-marker";
import type { VirtualList } from "./list-virtualizer";
import { NewMailMarker } from "./new-mail-marker";
import { EdgeRow, ThreadListItem } from "./thread-list-item";
import { rowAt } from "./use-thread-pages";
import styles from "./thread-list.module.css";

// The key that brings new mail in; the marker shows it.
export const REVEAL_KEYS: readonly Chord[] = [{ key: "." }];

interface GridProps {
  locale: Locale;
  today: number;
  scrollerRef: RefObject<HTMLDivElement | null>;
  list: VirtualList;
  rows: ReadonlyMap<number, ListRow[]>;
  rowCount: number;
  cursor: ListCursor;
  openThreadId: string | null;
  onKeyDown: (event: KeyboardEvent<HTMLElement>) => void;
  // A scroll or a click in the grid: the user acted in the list.
  onAct: () => void;
  // A click on a row opens it; the row's place is read off the row.
  onOpen: (index: number) => void;
}

// The row a click landed in, by its place in the list; null off a row.
function clickedRow(event: MouseEvent<HTMLElement>): number | null {
  const row = event.target instanceof Element ? event.target.closest("[data-index]") : null;
  const at = row instanceof HTMLElement ? row.dataset["index"] : undefined;
  return at === undefined ? null : Number(at);
}

// The rows in view, each placed by its offset; a row whose page is on
// its way is still, and so are the rows past the synced edge.
function Grid(props: GridProps) {
  const { locale, today, scrollerRef, list, rows, rowCount, cursor, openThreadId } = props;
  const onClick = (event: MouseEvent<HTMLElement>): void => {
    props.onAct();
    const index = clickedRow(event);
    if (index !== null) {
      props.onOpen(index);
    }
  };
  return (
    <div
      ref={scrollerRef}
      role="grid"
      tabIndex={-1}
      aria-label={m.list_label({}, { locale })}
      aria-rowcount={rowCount}
      className={styles.scroller}
      onKeyDown={props.onKeyDown}
      onScroll={props.onAct}
      onClick={onClick}
    >
      <div className={styles.sizer} style={{ blockSize: `${String(list.totalSize)}px` }}>
        {list.items.map((item) => {
          if (item.index >= rowCount) {
            return <EdgeRow key={item.key} placement={item} index={item.index} />;
          }
          const row = rowAt(rows, item.index);
          return (
            <ThreadListItem
              key={item.key}
              locale={locale}
              today={today}
              row={row}
              index={item.index}
              stop={item.index === cursor.active}
              selection={row !== undefined && row.threadId === openThreadId ? "selected" : "none"}
              start={item.start}
              size={item.size}
              onPlace={cursor.place}
            />
          );
        })}
      </div>
    </div>
  );
}

export interface ViewportProps extends Omit<GridProps, "onAct"> {
  markerRef: RefObject<HTMLDivElement | null>;
  marker: ListMarker;
  // New mail waiting, which the marker counts.
  pending: number;
  onReveal: () => void;
}

// The rows with the marker floating over them.
export function Viewport(props: ViewportProps) {
  const { locale, markerRef, marker, pending, onReveal, ...grid } = props;
  return (
    <div className={styles.viewport}>
      {marker.shown && (
        <NewMailMarker
          ref={markerRef}
          locale={locale}
          count={pending}
          keys={REVEAL_KEYS}
          onReveal={onReveal}
        />
      )}
      <Grid {...grid} locale={locale} onAct={marker.dismiss} />
    </div>
  );
}
