// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useSyncExternalStore } from "react";

// The width at which the sidebar joins the content; the stylesheet
// carries the same breakpoint.
const SIDEBAR_MIN_WIDTH_PX = 720;
const SIDEBAR_QUERY = `(min-width: ${String(SIDEBAR_MIN_WIDTH_PX)}px)`;

function subscribe(onChange: () => void): () => void {
  const media = window.matchMedia(SIDEBAR_QUERY);
  media.addEventListener("change", onChange);
  return () => {
    media.removeEventListener("change", onChange);
  };
}

function sidebarShown(): boolean {
  return window.matchMedia(SIDEBAR_QUERY).matches;
}

export function useSidebarShown(): boolean {
  return useSyncExternalStore(subscribe, sidebarShown, () => false);
}
