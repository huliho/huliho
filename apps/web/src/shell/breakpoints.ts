// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useSyncExternalStore } from "react";

export type Layout = "phone" | "tablet" | "desktop";

// The two shell breakpoints in CSS pixels: phone below the first, tablet
// from there, desktop from the second.
const TABLET_MIN_WIDTH_PX = 720;
const DESKTOP_MIN_WIDTH_PX = 1200;

const TABLET_QUERY = `(min-width: ${String(TABLET_MIN_WIDTH_PX)}px)`;
const DESKTOP_QUERY = `(min-width: ${String(DESKTOP_MIN_WIDTH_PX)}px)`;

function subscribe(onChange: () => void): () => void {
  const lists = [window.matchMedia(TABLET_QUERY), window.matchMedia(DESKTOP_QUERY)];
  for (const list of lists) {
    list.addEventListener("change", onChange);
  }
  return () => {
    for (const list of lists) {
      list.removeEventListener("change", onChange);
    }
  };
}

function layout(): Layout {
  if (window.matchMedia(DESKTOP_QUERY).matches) {
    return "desktop";
  }
  return window.matchMedia(TABLET_QUERY).matches ? "tablet" : "phone";
}

// Which of the three designed layouts the viewport gets.
export function useLayout(): Layout {
  return useSyncExternalStore(subscribe, layout, () => "phone");
}
