// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useSyncExternalStore } from "react";

function subscribe(onChange: () => void): () => void {
  window.addEventListener("online", onChange);
  window.addEventListener("offline", onChange);
  return () => {
    window.removeEventListener("online", onChange);
    window.removeEventListener("offline", onChange);
  };
}

function online(): boolean {
  return navigator.onLine;
}

// Whether the browser believes it has a network; a request's answer
// stays the truth.
export function useOnline(): boolean {
  return useSyncExternalStore(subscribe, online, () => true);
}
