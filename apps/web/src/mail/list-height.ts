// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useStoredSize } from "./stored-size";

const LIST_HEIGHT_KEY = "huliho-list-height";

// The reading pane below the list keeps at least this: its toolbar, the
// thread's title and one collapsed card.
export const PANE_MIN_HEIGHT_PX = 216;

// The rows the list keeps above the pane before anyone drags the seam,
// and the fewest it ever keeps; its height is that many rows under its
// header.
export const LIST_DEFAULT_ROWS = 6;
export const LIST_MIN_ROWS = 4;

// A height must leave the header something.
const LIST_HEIGHT_FLOOR_PX = 1;

// The list height this device chose, null while the design's default stands.
export function useListHeight(): [number | null, (next: number | null) => void] {
  return useStoredSize(LIST_HEIGHT_KEY, LIST_HEIGHT_FLOOR_PX);
}
