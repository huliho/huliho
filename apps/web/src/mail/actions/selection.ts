// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { WINDOW_SIZE } from "@huliho/core";
import type { ListRow } from "@huliho/core";
import { useState } from "react";

import { rowAt } from "../use-thread-pages";

// The threads picked for a bulk action, each by its row as last seen,
// so one whose page left the list keeps its place in the pick.
interface Picks {
  rows: ReadonlyMap<string, ListRow>;
  // The thread of the last row toggled, where a run with Shift starts.
  anchor: string | null;
  // Whether the mode stands: a pick made, or Select pressed before one.
  open: boolean;
}

const NONE: Picks = { rows: new Map(), anchor: null, open: false };

// One picked thread with where its row stands in the list, null for a
// row no page holds.
interface Picked {
  row: ListRow;
  index: number | null;
}

export interface Selection {
  // Whether the mode stands, with or without a pick.
  open: boolean;
  count: number;
  has: (threadId: string) => boolean;
  // The picks, each read from the pages where they hold it.
  picked: () => Picked[];
  toggle: (row: ListRow) => void;
  // Picks every loaded row from the anchor, else from `from`, to `index`.
  extend: (index: number, from: number) => void;
  start: () => void;
  clear: () => void;
  // Lets go of threads an action took out of the mailbox.
  drop: (threadIds: readonly string[]) => void;
}

// Where each loaded row stands, by its thread.
function placesOf(rows: ReadonlyMap<number, ListRow[]>): Map<string, Picked> {
  const places = new Map<string, Picked>();
  for (const [page, list] of rows) {
    list.forEach((row, offset) => {
      places.set(row.threadId, { row, index: page * WINDOW_SIZE + offset });
    });
  }
  return places;
}

// The picks without some threads. A mode whose last pick went closes;
// one Select opened stays until its first pick goes.
function without(picks: Picks, threadIds: readonly string[]): Picks {
  const rows = new Map(picks.rows);
  for (const id of threadIds) {
    rows.delete(id);
  }
  return { ...picks, rows, open: rows.size > 0 || (picks.open && picks.rows.size === 0) };
}

// The loaded rows from `start` to `end` either way, added to `rows`.
function withRun(
  rows: ReadonlyMap<string, ListRow>,
  pages: ReadonlyMap<number, ListRow[]>,
  start: number,
  end: number,
): Map<string, ListRow> {
  const next = new Map(rows);
  for (let at = Math.min(start, end); at <= Math.max(start, end); at += 1) {
    const row = rowAt(pages, at);
    if (row !== undefined) {
      next.set(row.threadId, row);
    }
  }
  return next;
}

// The rows picked for a bulk action and the mode they stand in. The
// mode closes when its last pick goes, on Close and after an action.
export function useSelection(pages: ReadonlyMap<number, ListRow[]>): Selection {
  const [picks, setPicks] = useState<Picks>(NONE);
  return {
    open: picks.open,
    count: picks.rows.size,
    has: (threadId) => picks.rows.has(threadId),
    picked: () => {
      const places = placesOf(pages);
      return [...picks.rows].map(([id, row]) => places.get(id) ?? { row, index: null });
    },
    toggle: (row) => {
      setPicks((held) =>
        held.rows.has(row.threadId)
          ? { ...without(held, [row.threadId]), anchor: row.threadId }
          : {
              rows: new Map(held.rows).set(row.threadId, row),
              anchor: row.threadId,
              open: true,
            },
      );
    },
    extend: (index, from) => {
      setPicks((held) => {
        const anchored = held.anchor === null ? undefined : placesOf(pages).get(held.anchor);
        const start = anchored?.index ?? from;
        const anchor = held.anchor ?? rowAt(pages, start)?.threadId ?? null;
        return { rows: withRun(held.rows, pages, start, index), anchor, open: true };
      });
    },
    start: () => {
      setPicks((held) => ({ ...held, open: true }));
    },
    clear: () => {
      setPicks(NONE);
    },
    drop: (threadIds) => {
      setPicks((held) => without(held, threadIds));
    },
  };
}
