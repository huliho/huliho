// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useStoredSize } from "./stored-size";

const LIST_WIDTH_KEY = "huliho-list-width";

// A pane never gets narrower than the smallest viewport the product supports.
export const PANE_MIN_WIDTH_PX = 320;

// The list's width before anyone drags the seam; the stylesheet carries
// the same value as a token.
export const LIST_WIDTH_DEFAULT_PX = 360;

// The list width this device chose, null while the design's default stands.
export function useListWidth(): [number | null, (next: number | null) => void] {
  return useStoredSize(LIST_WIDTH_KEY, PANE_MIN_WIDTH_PX);
}
