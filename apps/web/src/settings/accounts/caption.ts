// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow } from "@huliho/core";

import type { RetryOutcome } from "../../accounts/use-retry-account";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";

export interface RowCaption {
  text: string;
  // Warn for a stop waiting on someone, danger for a check that failed just now.
  tone: "muted" | "warn" | "danger";
  // Alert for a failed check, status for a pass; nothing for a standing state.
  live: "alert" | "status" | null;
}

// The row's state line: the stop it waits in or what the last retry did.
// A retry outcome only speaks for a row still stopped on its connection;
// a row the probe resumed meanwhile shows its own state.
export function captionOf(
  row: AccountRow,
  outcome: RetryOutcome | undefined,
  minutes: number,
  locale: Locale,
): RowCaption | null {
  if (row.stoppedCause === null) {
    return outcome === "resumed"
      ? { text: m.accounts_resumed({}, { locale }), tone: "muted", live: "status" }
      : null;
  }
  if (row.stoppedCause === "credentials") {
    return { text: m.accounts_expired({}, { locale }), tone: "warn", live: null };
  }
  if (outcome === "failed") {
    return { text: m.accounts_retry_failed({}, { locale }), tone: "danger", live: "alert" };
  }
  return outcome === "stillStopped"
    ? { text: m.accounts_still_stopped({ minutes }, { locale }), tone: "danger", live: "alert" }
    : { text: m.accounts_stopped({ minutes }, { locale }), tone: "warn", live: null };
}
