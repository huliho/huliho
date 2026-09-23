// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { JmapError } from "@huliho/core";
import type { JmapFailureCode, StopCause } from "@huliho/core";

// A failure as it crosses the worker boundary, where a thrown error
// loses its fields.
interface CacheFailure {
  code: JmapFailureCode;
  stopCause: StopCause | null;
  limit: string | null;
}

export type CacheResult<Value> = { ok: true; value: Value } | { ok: false; failure: CacheFailure };

// Anything but a named failure reads as the server not answering; the
// worker's console keeps the cause.
function failureOf(error: unknown): CacheFailure {
  if (error instanceof JmapError) {
    return { code: error.code, stopCause: error.stopCause, limit: error.limit };
  }
  console.error("cache: a call failed", error instanceof Error ? error.message : String(error));
  return { code: "unavailable", stopCause: null, limit: null };
}

export async function attempt<Value>(run: () => Promise<Value>): Promise<CacheResult<Value>> {
  try {
    return { ok: true, value: await run() };
  } catch (error) {
    return { ok: false, failure: failureOf(error) };
  }
}
