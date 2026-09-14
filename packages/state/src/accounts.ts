// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { fetchAccounts, fetchConsent } from "@huliho/core";
import type { ConsentOutcome } from "@huliho/core";
import { queryOptions, skipToken } from "@tanstack/react-query";

import { queryKeys } from "./keys";

// The shell guard reads the list on every visit; a return within this
// window reuses it.
const ACCOUNTS_STALE_MS = 30_000;

// The card asks every two seconds where a consent stands; the provider's
// window holds the user meanwhile, so a faster poll buys nothing.
const CONSENT_POLL_MS = 2_000;

export const accountsQueryOptions = queryOptions({
  queryKey: queryKeys.accounts,
  queryFn: fetchAccounts,
  staleTime: ACCOUNTS_STALE_MS,
  retry: false,
});

function settled(outcome: ConsentOutcome | undefined): boolean {
  return outcome !== undefined && outcome.status !== "pending";
}

// The poll behind an open consent window; null names no consent and
// asks nothing. Nothing is kept once the card moves on.
export function consentQueryOptions(id: string | null) {
  return queryOptions({
    queryKey: [...queryKeys.consent, id ?? ""] as const,
    queryFn: id === null ? skipToken : () => fetchConsent(id),
    gcTime: 0,
    retry: false,
    refetchInterval: (query) => (settled(query.state.data) ? false : CONSENT_POLL_MS),
  });
}
