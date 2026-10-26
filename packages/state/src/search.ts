// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MailCache } from "@huliho/core";
import { keepPreviousData, queryOptions } from "@tanstack/react-query";

import { queryKeys } from "./keys";

// A search is a snapshot: it stays as it landed until a cache message
// reads it again from the held pages, a failure is left to the user and
// one given up stays given up when the window comes back into focus.
const snapshot = {
  staleTime: Number.POSITIVE_INFINITY,
  retry: false,
  networkMode: "always",
  refetchOnWindowFocus: false,
} as const;

// One page of a search; `again` runs it anew on the server.
export function searchWindowQueryOptions(
  cache: MailCache,
  accountId: string,
  key: string,
  { locale, page, again = false }: { locale: string; page: number; again?: boolean },
) {
  return queryOptions({
    queryKey: queryKeys.search(accountId, key, locale, page),
    queryFn: ({ signal }) => cache.search(accountId, key, page, { locale, again, signal }),
    ...snapshot,
  });
}

// The senders whose name or address starts with the prefix; the last
// answer stands while the next one comes.
export function sendersQueryOptions(cache: MailCache, accountId: string, prefix: string) {
  return queryOptions({
    queryKey: queryKeys.sendersFor(accountId, prefix),
    queryFn: () => cache.senders(accountId, prefix),
    placeholderData: keepPreviousData,
    ...snapshot,
  });
}
