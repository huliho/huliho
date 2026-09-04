// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { QueryClient } from "@tanstack/react-query";

export interface Pending {
  flush: (keepalive: boolean) => void;
  // Takes the rows out of the cache again after fresh data put them back.
  reapply: () => void;
}

const pending = new Set<Pending>();

export function trackPending(entry: Pending): () => void {
  pending.add(entry);
  return () => {
    pending.delete(entry);
  };
}

function reapplyPending(): void {
  for (const entry of pending) {
    entry.reapply();
  }
}

// A removal waiting behind its undo toast must not die with the page.
export function flushPendingOnPageHide(target: Window = window): () => void {
  const onPageHide = (): void => {
    for (const entry of pending) {
      entry.flush(true);
    }
    pending.clear();
  };
  target.addEventListener("pagehide", onPageHide);
  return () => {
    target.removeEventListener("pagehide", onPageHide);
  };
}

// The server still lists a row until its removal is sent, so every fetched
// answer gets the pending removals applied again. Manual cache writes are
// skipped: the reapply itself is one.
export function reapplyPendingAfterFetch(queryClient: QueryClient): () => void {
  return queryClient.getQueryCache().subscribe((event) => {
    if (
      event.type === "updated" &&
      event.action.type === "success" &&
      event.action.manual !== true
    ) {
      reapplyPending();
    }
  });
}
