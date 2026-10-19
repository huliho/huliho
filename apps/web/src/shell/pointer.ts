// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useSyncExternalStore } from "react";

// A touchscreen anywhere on the device, as the touch density reads it.
const COARSE_QUERY = "(any-pointer: coarse)";

function subscribe(onChange: () => void): () => void {
  const list = window.matchMedia(COARSE_QUERY);
  list.addEventListener("change", onChange);
  return () => {
    list.removeEventListener("change", onChange);
  };
}

function coarse(): boolean {
  return window.matchMedia(COARSE_QUERY).matches;
}

// Whether the device has a touchscreen, for a control that only touch needs.
export function useCoarsePointer(): boolean {
  return useSyncExternalStore(subscribe, coarse, () => false);
}
