// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MailCache } from "@huliho/core";
import { queryOptions } from "@tanstack/react-query";

import { queryKeys } from "./keys";

// The cache is the truth: a query stays fresh until a cache message
// invalidates it, and a failure waits for the user rather than a retry.
const settled = { staleTime: Number.POSITIVE_INFINITY, retry: false } as const;

export function mailboxesQueryOptions(cache: MailCache, accountId: string) {
  return queryOptions({
    queryKey: queryKeys.mailboxes(accountId),
    queryFn: () => cache.mailboxes(accountId),
    ...settled,
  });
}

export function threadWindowQueryOptions(
  cache: MailCache,
  accountId: string,
  mailboxId: string,
  page: number,
) {
  return queryOptions({
    queryKey: queryKeys.window(accountId, mailboxId, page),
    queryFn: () => cache.window(accountId, mailboxId, page),
    ...settled,
  });
}

export function threadQueryOptions(cache: MailCache, accountId: string, threadId: string) {
  return queryOptions({
    queryKey: queryKeys.thread(accountId, threadId),
    queryFn: () => cache.thread(accountId, threadId),
    ...settled,
  });
}
