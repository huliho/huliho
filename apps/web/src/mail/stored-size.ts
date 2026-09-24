// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useState } from "react";

function stored(key: string, floor: number): number | null {
  const held = localStorage.getItem(key);
  const size = Number(held);
  return held !== null && Number.isInteger(size) && size >= floor ? size : null;
}

// A pane size this device chose, in CSS pixels under `key`; null while
// the design's default stands. A stored value below `floor` or not a
// whole number reads as the default.
export function useStoredSize(
  key: string,
  floor: number,
): [number | null, (next: number | null) => void] {
  const [size, setSize] = useState(() => stored(key, floor));
  const choose = (next: number | null): void => {
    setSize(next);
    if (next === null) {
      localStorage.removeItem(key);
    } else {
      localStorage.setItem(key, String(next));
    }
  };
  return [size, choose];
}
