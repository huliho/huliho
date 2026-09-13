// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { fetchAccounts } from "@huliho/core";
import { queryOptions } from "@tanstack/react-query";

import { queryKeys } from "./keys";

// The shell guard reads the list on every visit; a return within this
// window reuses it.
const ACCOUNTS_STALE_MS = 30_000;

export const accountsQueryOptions = queryOptions({
  queryKey: queryKeys.accounts,
  queryFn: fetchAccounts,
  staleTime: ACCOUNTS_STALE_MS,
  retry: false,
});
