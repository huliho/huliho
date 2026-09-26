// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { vi } from "vitest";

// Whether the width queries match: every one at the desktop width, none on a phone.
export interface WidthQueries {
  wide: boolean;
}

// jsdom answers no media query; the shell's layout hook reads two. The
// stub answers them from `queries`, which a test may flip.
export function stubWidthQueries(queries: WidthQueries): void {
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: queries.wide,
    media: query,
    addEventListener() {
      return undefined;
    },
    removeEventListener() {
      return undefined;
    },
  }));
}
