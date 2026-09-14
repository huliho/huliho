// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useEffect, useRef } from "react";
import type { RefObject } from "react";

export type FocusBefore<TRow> = (id: string, actionable: (row: TRow) => boolean) => void;

// The pressed control leaves with its row, so focus moves on first: the
// next staying row, else the previous, else the last resort or the list.
// Rows already leaving are remembered, so a quick second keypress never
// lands on one.
export function useRowFocus<TRow extends { id: string }>(
  rows: TRow[],
  list: RefObject<HTMLUListElement | null>,
  controlOf: (id: string) => string,
  lastResort?: RefObject<HTMLElement | null>,
): FocusBefore<TRow> {
  const leaving = useRef(new Set<string>());
  useEffect(() => {
    for (const id of leaving.current) {
      if (!rows.some((row) => row.id === id)) {
        leaving.current.delete(id);
      }
    }
  }, [rows]);
  return (id, actionable) => {
    leaving.current.add(id);
    const index = rows.findIndex((row) => row.id === id);
    const staying = (row: TRow): boolean => actionable(row) && !leaving.current.has(row.id);
    const neighbor = rows.slice(index + 1).find(staying) ?? rows.slice(0, index).findLast(staying);
    const target =
      neighbor === undefined
        ? (lastResort?.current ?? list.current)
        : list.current?.querySelector<HTMLElement>(controlOf(neighbor.id));
    target?.focus();
  };
}
