// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow } from "@huliho/core";
import { useState } from "react";
import type { KeyboardEvent, MouseEvent, PointerEvent, RefObject } from "react";

import type { Chord } from "../commands/keys";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { BulkSlot } from "./actions/bulk-slot";
import type { BulkHandle } from "./actions/bulk-handle";
import { useLongPress } from "./actions/long-press";
import { ROW_ACTIONS } from "./actions/row-actions";
import type { RowActionsHandle } from "./actions/row-actions";
import type { ListCursor } from "./list-cursor";
import type { ListMarker } from "./list-marker";
import type { VirtualList } from "./list-virtualizer";
import { NewMailMarker } from "./new-mail-marker";
import { EdgeRow, ROW_CHECK, ThreadListItem } from "./thread-list-item";
import type { Check } from "./thread-list-item";
import { rowAt } from "./use-thread-pages";
import styles from "./thread-list.module.css";

// The key that brings new mail in; the marker shows it.
export const REVEAL_KEYS: readonly Chord[] = [{ key: "." }];

// What the grid asks of the selection: whether rows can be picked, the
// mode with its picks and the two ways to change them.
export interface PickHandle {
  offered: boolean;
  open: boolean;
  has: (threadId: string) => boolean;
  toggle: (row: ListRow) => void;
  extend: (index: number, from: number) => void;
}

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
  picks: PickHandle;
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

function inside(target: EventTarget, selector: string): boolean {
  return target instanceof Element && target.closest(selector) !== null;
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

// The row's check: on or off while a selection stands, before that an
// empty box under the pointer or under a resting touch.
function checkOf(picks: PickHandle, row: ListRow | undefined, pointed: boolean): Check {
  if (!picks.offered || row === undefined) {
    return "none";
  }
  if (picks.open) {
    return picks.has(row.threadId) ? "on" : "off";
  }
  return pointed ? "offered" : "none";
}

// What the pointer does in the grid: the row it hovers, the row a touch
// rests on and the handlers that tell a press on a row's controls, a
// run with Shift, a tap while a selection stands and a plain click
// apart.
function useGridPointer(props: GridProps) {
  const { rows, cursor, picks } = props;
  const { hovered, onPointerOver, onPointerLeave } = useHoveredRow();
  const press = useLongPress(
    rowOf,
    (index) => {
      const row = rowAt(rows, index);
      if (row !== undefined) {
        picks.toggle(row);
      }
    },
    picks.offered,
  );
  const onClick = (event: MouseEvent<HTMLElement>): void => {
    props.onAct();
    const index = rowOf(event.target);
    const row = index === null ? undefined : rowAt(rows, index);
    if (index === null || inside(event.target, `${ROW_ACTIONS}, ${ROW_CHECK}`)) {
      return;
    }
    if (picks.offered && row !== undefined && event.shiftKey) {
      picks.extend(index, cursor.active);
    } else if (picks.open && row !== undefined && press.touched()) {
      picks.toggle(row);
    } else {
      props.onOpen(index);
    }
  };
  const handlers = {
    onClickCapture: (event: MouseEvent<HTMLElement>): void => {
      if (press.takeClick()) {
        event.preventDefault();
        event.stopPropagation();
      }
    },
    onClick,
    // A run picked with Shift selects no text and leaves the focus.
    onMouseDown: (event: MouseEvent<HTMLElement>): void => {
      if (event.shiftKey && picks.offered) {
        event.preventDefault();
      }
    },
    onContextMenu: (event: MouseEvent<HTMLElement>): void => {
      if (press.touched()) {
        event.preventDefault();
      }
    },
    onPointerDown: press.onPointerDown,
    onPointerUp: press.onPointerEnd,
    onPointerCancel: press.onPointerEnd,
    onPointerOver,
    onPointerLeave: (): void => {
      onPointerLeave();
      press.onPointerEnd();
    },
  };
  return { hovered, pressing: press.pressing, handlers };
}

type GridPointer = ReturnType<typeof useGridPointer>;

// One placed row: still past the synced edge, else the thread's row with
// its check and, under the pointer or at the cursor, its actions.
function drawn(props: GridProps, pointer: GridPointer, item: VirtualList["items"][number]) {
  const { cursor, picks } = props;
  if (item.index >= props.rowCount) {
    return <EdgeRow key={item.key} placement={item} index={item.index} />;
  }
  const row = rowAt(props.rows, item.index);
  const pointed = item.index === pointer.hovered || item.index === pointer.pressing;
  const acts =
    row !== undefined && (item.index === pointer.hovered || item.index === cursor.active);
  return (
    <ThreadListItem
      key={item.key}
      locale={props.locale}
      today={props.today}
      row={row}
      index={item.index}
      stop={item.index === cursor.active}
      open={row !== undefined && row.threadId === props.openThreadId}
      check={checkOf(picks, row, pointed)}
      pressing={item.index === pointer.pressing}
      actions={acts ? props.rowActions : null}
      start={item.start}
      size={item.size}
      onPlace={cursor.place}
      onCheck={(shift) => {
        if (row !== undefined && shift) {
          picks.extend(item.index, cursor.active);
        } else if (row !== undefined) {
          picks.toggle(row);
        }
      }}
    />
  );
}

// The rows in view, each placed by its offset; a row whose page is on
// its way is still, and so are the rows past the synced edge. A press on
// a row's actions or on its check acts and opens nothing.
function Grid(props: GridProps) {
  const { locale, scrollerRef, list, rowCount, picks } = props;
  const pointer = useGridPointer(props);
  return (
    <div
      ref={scrollerRef}
      role="grid"
      tabIndex={-1}
      aria-label={m.list_label({}, { locale })}
      aria-rowcount={rowCount}
      aria-multiselectable={picks.open || undefined}
      className={styles.scroller}
      onKeyDown={props.onKeyDown}
      onScroll={props.onAct}
      {...pointer.handlers}
    >
      <div className={styles.sizer} style={{ blockSize: `${String(list.totalSize)}px` }}>
        {list.items.map((item) => drawn(props, pointer, item))}
      </div>
    </div>
  );
}

export interface ViewportProps extends Omit<GridProps, "onAct"> {
  markerRef: RefObject<HTMLDivElement | null>;
  marker: ListMarker;
  // New mail waiting, which the marker counts.
  pending: number;
  // The bulk bar's handle while a selection stands.
  bulk: BulkHandle | null;
  onReveal: () => void;
}

// The rows with the marker floating over them, or the bulk bar while a
// selection stands.
export function Viewport(props: ViewportProps) {
  const { locale, markerRef, marker, pending, bulk, onReveal, ...grid } = props;
  return (
    <div className={styles.viewport}>
      {bulk === null && marker.shown && (
        <NewMailMarker
          ref={markerRef}
          locale={locale}
          count={pending}
          keys={REVEAL_KEYS}
          onReveal={onReveal}
        />
      )}
      {bulk !== null && <BulkSlot locale={locale} handle={bulk} />}
      <Grid {...grid} locale={locale} onAct={marker.dismiss} />
    </div>
  );
}
