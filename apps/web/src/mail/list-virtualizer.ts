// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { defaultRangeExtractor, useVirtualizer } from "@tanstack/react-virtual";
import type { Range, VirtualItem } from "@tanstack/react-virtual";
import { useLayoutEffect, useRef } from "react";
import type { RefObject } from "react";

import { pageOf } from "./use-thread-pages";

// Rows rendered beyond the visible ones on either side.
const LIST_OVERSCAN = 3;
// The page this many rows past the rows in view is asked for ahead of
// time: what a fast fling covers while a page is on its way, so the rows
// land loaded instead of still.
const PREFETCH_ROWS = 40;

export interface VirtualListOptions {
  scrollerRef: RefObject<HTMLDivElement | null>;
  rowHeight: number;
  // The rows the list has, then the still rows drawn after them.
  rowCount: number;
  edgeRows: number;
  // The row that carries the tab stop; it stays rendered while it scrolls out.
  active: number;
  // Told which pages the rows in view touch, as "first-last".
  onSpan: (span: string) => void;
}

export interface VirtualList {
  items: VirtualItem[];
  totalSize: number;
  scrollTo: (index: number) => void;
}

// The pages a span names, plus the first page and the one the cursor
// was last moved to, since that row stays rendered out of view.
export function pagesFor(span: string, pinned: number): number[] {
  const [from = 0, through = 0] = span.split("-").map(Number);
  const pages = new Set([0, pinned]);
  for (let page = from; page <= through; page += 1) {
    pages.add(page);
  }
  return [...pages].toSorted((first, second) => first - second);
}

// The pages the rows in view touch, and the ones a fling would reach
// next, never past the last row.
function spanOf(range: { startIndex: number; endIndex: number }, rowCount: number): string {
  const last = Math.max(0, rowCount - 1);
  const from = pageOf(Math.min(Math.max(0, range.startIndex - PREFETCH_ROWS), last));
  const through = pageOf(Math.min(range.endIndex + PREFETCH_ROWS, last));
  return `${String(from)}-${String(through)}`;
}

function withActive(indexes: number[], active: number, rowCount: number): number[] {
  if (rowCount > 0 && !indexes.includes(active)) {
    indexes.push(active);
    indexes.sort((one, other) => one - other);
  }
  return indexes;
}

// The rows in view over the whole list's height, each keyed by its
// place, so a row keeps its element while its page lands.
export function useListVirtualizer(options: VirtualListOptions): VirtualList {
  const { scrollerRef, rowHeight, rowCount, edgeRows, active, onSpan } = options;
  // The span last told; a scroll that stays inside it tells nothing, so
  // the frame carries no second render for an unchanged state.
  const spanRef = useRef("");
  const virtualizer = useVirtualizer({
    count: rowCount + edgeRows,
    getScrollElement: () => scrollerRef.current,
    estimateSize: () => rowHeight,
    overscan: LIST_OVERSCAN,
    rangeExtractor: (range: Range) => withActive(defaultRangeExtractor(range), active, rowCount),
    onChange: (instance) => {
      if (instance.range === null) {
        return;
      }
      const span = spanOf(instance.range, rowCount);
      if (span !== spanRef.current) {
        spanRef.current = span;
        onSpan(span);
      }
    },
  });
  // The measurements rest on the row height; a density change under a
  // mounted list moves the rows to the new one.
  const measuredRef = useRef(rowHeight);
  useLayoutEffect(() => {
    if (measuredRef.current !== rowHeight) {
      measuredRef.current = rowHeight;
      virtualizer.measure();
    }
  }, [virtualizer, rowHeight]);
  return {
    items: virtualizer.getVirtualItems(),
    totalSize: virtualizer.getTotalSize(),
    scrollTo: (index) => {
      virtualizer.scrollToIndex(index, { align: "auto" });
    },
  };
}
