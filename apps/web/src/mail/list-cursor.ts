// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow } from "@huliho/core";
import { useLayoutEffect, useRef, useState } from "react";
import type { KeyboardEvent, RefObject } from "react";

import { indexOf, indexOfThread, rowAt } from "./use-thread-pages";

// The row that carries the roving tab stop, by place and by id, so it
// stays on its thread when the rows above it change.
interface Cursor {
  index: number;
  id: string | null;
}

interface CursorOptions {
  scrollerRef: RefObject<HTMLElement | null>;
  rows: ReadonlyMap<number, ListRow[]>;
  rowCount: number;
  // The thread open beside the list; the focus returns to the cursor's
  // row when it closes and the focus went with it.
  openThreadId: string | null;
  // Told of every move by key, with the row it went to.
  onMove: (index: number) => void;
}

export interface ListCursor {
  active: number;
  // A row took focus on its own, by pointer or by Tab.
  place: (index: number) => void;
  // Moves the stop and the focus to a row, scrolling it into view.
  moveTo: (index: number, scrollTo: (index: number) => void) => void;
  // Brings the focus to the cursor's row, now or once that row renders.
  focus: () => void;
}

function clampIndex(index: number, count: number): number {
  return Math.min(Math.max(0, index), Math.max(0, count - 1));
}

// The cursor's row when the focus stands on another row of the grid,
// null when the focus is elsewhere or already there.
function activeIfFocused(scroller: HTMLElement, active: number): number | null {
  const focused = document.activeElement;
  if (!(focused instanceof HTMLElement) || !scroller.contains(focused)) {
    return null;
  }
  const at = focused.dataset["index"];
  return at !== undefined && Number(at) !== active ? active : null;
}

// Where an arrow, Home or End takes the cursor; null for any other key.
function keyTarget(key: string, index: number, count: number): number | null {
  switch (key) {
    case "ArrowDown":
      return index + 1;
    case "ArrowUp":
      return index - 1;
    case "Home":
      return 0;
    case "End":
      return count - 1;
    default:
      return null;
  }
}

// The handler for the grid's keys: an arrow, Home or End moves the
// cursor and Enter opens its row.
export function keyHandler(
  cursor: ListCursor,
  rowCount: number,
  scrollTo: (index: number) => void,
  open: () => void,
): (event: KeyboardEvent<HTMLElement>) => void {
  return (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      open();
      return;
    }
    const target = keyTarget(event.key, cursor.active, rowCount);
    if (target === null) {
      return;
    }
    event.preventDefault();
    cursor.moveTo(target, scrollTo);
  };
}

// Whether the focus is on nothing: what a closed thread leaves behind
// when the focus stood inside it, and a row that left with it.
export function focusLost(): boolean {
  return document.activeElement === null || document.activeElement === document.body;
}

// The row of the grid that holds the focus, if one does.
function focusedRow(scroller: HTMLElement): HTMLElement | null {
  const focused = document.activeElement;
  return focused instanceof HTMLElement && scroller.contains(focused) ? focused : null;
}

// The cursor's row once the row that held the focus left the list, or
// when the focus stands on another row; null where it stays.
function keptIndex(scroller: HTMLElement, active: number, dropped: boolean): number | null {
  return dropped ? active : activeIfFocused(scroller, active);
}

// Focuses the row at `index` if it renders. A move scrolled already; a
// row that shifted under the focus is brought into view.
function focusRow(scroller: HTMLElement, index: number | null, moved: boolean): boolean {
  const row =
    index === null ? null : scroller.querySelector<HTMLElement>(`[data-index="${String(index)}"]`);
  if (!(row instanceof HTMLElement)) {
    return false;
  }
  row.focus({ preventScroll: moved });
  return true;
}

// The row a closed thread returns the focus to: the cursor's, or the
// thread's own row while nothing placed the cursor, as after a thread
// opened from the address.
function returnRow(
  rows: ReadonlyMap<number, ListRow[]>,
  cursor: Cursor,
  threadId: string,
  active: number,
): number {
  return cursor.id === null ? (indexOfThread(rows, threadId) ?? active) : active;
}

// The one tab stop of the grid and the focus that follows it: the arrow
// keys, Home, End, j and k move it; a click or Tab places it.
export function useCursor(options: CursorOptions): ListCursor {
  const { scrollerRef, rows, rowCount, openThreadId, onMove } = options;
  const [cursor, setCursor] = useState<Cursor>({ index: 0, id: null });
  const pendingFocusRef = useRef<number | null>(null);
  const openRef = useRef(openThreadId);
  const heldRef = useRef<HTMLElement | null>(null);
  const found = cursor.id === null ? null : indexOf(rows, cursor.id);
  const active = clampIndex(found ?? cursor.index, rowCount);
  // The focus follows the cursor: to the row a move asked for, to the
  // row the cursor's thread stands in once rows above it changed, back
  // to the cursor's row when the thread it opened closes and to it when
  // the row that held the focus left the list.
  useLayoutEffect(() => {
    const scroller = scrollerRef.current;
    if (scroller === null) {
      return;
    }
    const closedId = openThreadId === null && focusLost() ? openRef.current : null;
    openRef.current = openThreadId;
    const dropped = heldRef.current?.isConnected === false && focusLost();
    const moved = pendingFocusRef.current !== null;
    const wanted =
      pendingFocusRef.current ??
      (closedId === null
        ? keptIndex(scroller, active, dropped)
        : returnRow(rows, cursor, closedId, active));
    if (focusRow(scroller, wanted, moved)) {
      pendingFocusRef.current = null;
    }
    heldRef.current = focusedRow(scroller);
  });
  return {
    active,
    place: (index) => {
      setCursor({ index, id: rowAt(rows, index)?.id ?? null });
    },
    moveTo: (index, scrollTo) => {
      const next = clampIndex(index, rowCount);
      pendingFocusRef.current = next;
      setCursor({ index: next, id: rowAt(rows, next)?.id ?? null });
      scrollTo(next);
      onMove(next);
    },
    focus: () => {
      pendingFocusRef.current = active;
      setCursor({ index: active, id: rowAt(rows, active)?.id ?? null });
    },
  };
}
