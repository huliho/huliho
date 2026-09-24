// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow } from "@huliho/core";
import { useLayoutEffect, useRef, useState } from "react";
import type { KeyboardEvent, RefObject } from "react";

import { indexOf, rowAt } from "./use-thread-pages";

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

// The handler for the grid's keys: an arrow, Home or End moves the cursor.
export function keyHandler(
  cursor: ListCursor,
  rowCount: number,
  scrollTo: (index: number) => void,
): (event: KeyboardEvent<HTMLElement>) => void {
  return (event) => {
    const target = keyTarget(event.key, cursor.active, rowCount);
    if (target === null) {
      return;
    }
    event.preventDefault();
    cursor.moveTo(target, scrollTo);
  };
}

// The one tab stop of the grid and the focus that follows it: the arrow
// keys, Home, End, j and k move it; a click or Tab places it.
export function useCursor({ scrollerRef, rows, rowCount, onMove }: CursorOptions): ListCursor {
  const [cursor, setCursor] = useState<Cursor>({ index: 0, id: null });
  const pendingFocusRef = useRef<number | null>(null);
  const found = cursor.id === null ? null : indexOf(rows, cursor.id);
  const active = clampIndex(found ?? cursor.index, rowCount);
  // The focus follows the cursor: to the row a move asked for, and to
  // the row the cursor's thread stands in once rows above it changed.
  useLayoutEffect(() => {
    const scroller = scrollerRef.current;
    if (scroller === null) {
      return;
    }
    const moved = pendingFocusRef.current !== null;
    const wanted = pendingFocusRef.current ?? activeIfFocused(scroller, active);
    if (wanted === null) {
      return;
    }
    const row = scroller.querySelector<HTMLElement>(`[data-index="${String(wanted)}"]`);
    if (row instanceof HTMLElement) {
      pendingFocusRef.current = null;
      // A move scrolled already; a row that shifted under the focus is brought into view.
      row.focus({ preventScroll: moved });
    }
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
