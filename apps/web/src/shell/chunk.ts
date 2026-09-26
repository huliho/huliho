// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// A promise that carries its outcome on itself, the shape `use` reads
// without suspending once it settled.
interface Settled<T> extends Promise<T> {
  status?: "pending" | "fulfilled" | "rejected";
  value?: T;
  reason?: unknown;
}

async function settle<T>(loading: Settled<T>): Promise<void> {
  try {
    loading.value = await loading;
    loading.status = "fulfilled";
  } catch (reason: unknown) {
    loading.reason = reason;
    loading.status = "rejected";
  }
}

// A chunk fetched once, on demand: the first call starts the import and
// every call answers the same promise, its outcome written onto it, so
// a render that reads it through `use` after it landed does not wait.
// A refused import is handed over once, to the render that throws it
// to its boundary; the call after that starts the import afresh, so
// Try again there fetches the chunk again.
export function chunk<T>(load: () => Promise<T>): () => Promise<T> {
  let loading: Settled<T> | null = null;
  return () => {
    if (loading === null) {
      loading = load();
      loading.status = "pending";
      void settle(loading);
    }
    const current = loading;
    if (current.status === "rejected") {
      loading = null;
    }
    return current;
  };
}
