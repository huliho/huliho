// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow } from "@huliho/core";
import { useState } from "react";
import type { KeyboardEvent, MouseEvent, PointerEvent, RefObject } from "react";

import type { Chord } from "../commands/keys";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { ROW_ACTIONS } from "./actions/row-actions";
import type { RowActionsHandle } from "./actions/row-actions";
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
  rowActions: RowActionsHandle;
  onKeyDown: (event: KeyboardEvent<HTMLElement>) => void;
  // A scroll or a click in the grid: the user acted in the list.
  onAct: () => void;
  // A click on a row opens it; the row's place is read off the row.
  onOpen: (index: number) => void;
}

// The row an event landed in, by its place in the list; null off a row.
function rowOf(target: EventTarget): number | null {
  const row = target instanceof Element ? target.closest("[data-index]") : null;
  const at = row instanceof HTMLElement ? row.dataset["index"] : undefined;
  return at === undefined ? null : Number(at);
}

// The row under a pointer that can hover; a touch never hovers.
function useHoveredRow() {
  const [hovered, setHovered] = useState<number | null>(null);
  return {
    hovered,
    onPointerOver: (event: PointerEvent<HTMLElement>): void => {
      if (event.pointerType !== "touch") {
        setHovered(rowOf(event.target));
      }
    },
    onPointerLeave: (): void => {
      setHovered(null);
    },
  };
}

// The rows in view, each placed by its offset; a row whose page is on
// its way is still, and so are the rows past the synced edge. The row
// under the pointer and the cursor's row carry the thread's actions; a
// press on one of them acts and opens nothing.
function Grid(props: GridProps) {
  const { locale, today, scrollerRef, list, rows, rowCount, cursor, openThreadId } = props;
  const { hovered, onPointerOver, onPointerLeave } = useHoveredRow();
  const onClick = (event: MouseEvent<HTMLElement>): void => {
    props.onAct();
    const index = rowOf(event.target);
    const acted = event.target instanceof Element && event.target.closest(ROW_ACTIONS) !== null;
    if (index !== null && !acted) {
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
      onPointerOver={onPointerOver}
      onPointerLeave={onPointerLeave}
    >
      <div className={styles.sizer} style={{ blockSize: `${String(list.totalSize)}px` }}>
        {list.items.map((item) => {
          if (item.index >= rowCount) {
            return <EdgeRow key={item.key} placement={item} index={item.index} />;
          }
          const row = rowAt(rows, item.index);
          const acts =
            row !== undefined && (item.index === hovered || item.index === cursor.active);
          return (
            <ThreadListItem
              key={item.key}
              locale={locale}
              today={today}
              row={row}
              index={item.index}
              stop={item.index === cursor.active}
              selection={row !== undefined && row.threadId === openThreadId ? "selected" : "none"}
              actions={acts ? props.rowActions : null}
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
