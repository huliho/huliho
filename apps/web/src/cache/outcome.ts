// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { JmapError, MethodFailure } from "@huliho/core";
import type { JmapFailureCode, StopCause } from "@huliho/core";

// A failure as it crosses the worker boundary, where a thrown error
// loses its fields; `method` names the error a method answered (RFC 8620
// section 3.6.2), null for every other failure.
interface CacheFailure {
  code: JmapFailureCode;
  stopCause: StopCause | null;
  limit: string | null;
  method: { type: string; call: string } | null;
}

export type CacheResult<Value> = { ok: true; value: Value } | { ok: false; failure: CacheFailure };

function given(error: unknown): boolean {
  return error instanceof DOMException && error.name === "AbortError";
}

// Anything but a named failure reads as the server not answering; the
// worker's console keeps the cause, except for a call its caller gave up.
function failureOf(error: unknown): CacheFailure {
  if (error instanceof JmapError) {
    return { code: error.code, stopCause: error.stopCause, limit: error.limit, method: null };
  }
  const method = error instanceof MethodFailure ? { type: error.type, call: error.call } : null;
  if (!given(error)) {
    console.error("cache: a call failed", error instanceof Error ? error.message : String(error));
  }
  return { code: "unavailable", stopCause: null, limit: null, method };
}

export async function attempt<Value>(run: () => Promise<Value>): Promise<CacheResult<Value>> {
  try {
    return { ok: true, value: await run() };
  } catch (error) {
    return { ok: false, failure: failureOf(error) };
  }
}
