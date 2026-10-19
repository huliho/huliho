// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { vi } from "vitest";

// Whether the width queries match: every one at the desktop width, none
// on a phone. With `coarse` the device has a touchscreen as well.
export interface WidthQueries {
  wide: boolean;
  coarse?: boolean;
}

// jsdom answers no media query; the shell reads two for the width and
// one for the pointer. The stub answers the width queries from
// `queries`, which a test may flip, the pointer query from `coarse` and
// every other query with no match.
export function stubWidthQueries(queries: WidthQueries): void {
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches:
      (query.includes("min-width") && queries.wide) ||
      (query.includes("coarse") && queries.coarse === true),
    media: query,
    addEventListener() {
      return undefined;
    },
    removeEventListener() {
      return undefined;
    },
  }));
}
