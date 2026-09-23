// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useState } from "react";

const LIST_WIDTH_KEY = "huliho-list-width";

// A pane never gets narrower than the smallest viewport the product supports.
export const PANE_MIN_WIDTH_PX = 320;

// The list's width before anyone drags the seam; the stylesheet carries
// the same value as a token.
export const LIST_WIDTH_DEFAULT_PX = 360;

function storedListWidth(): number | null {
  const held = localStorage.getItem(LIST_WIDTH_KEY);
  const width = Number(held);
  return held !== null && Number.isInteger(width) && width >= PANE_MIN_WIDTH_PX ? width : null;
}

// The list width this device chose, null while the design's default stands.
export function useListWidth(): [number | null, (next: number | null) => void] {
  const [width, setWidth] = useState(storedListWidth);
  const choose = (next: number | null): void => {
    setWidth(next);
    if (next === null) {
      localStorage.removeItem(LIST_WIDTH_KEY);
    } else {
      localStorage.setItem(LIST_WIDTH_KEY, String(next));
    }
  };
  return [width, choose];
}
