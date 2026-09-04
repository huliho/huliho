// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { fetchSessions } from "@huliho/core";
import { queryOptions } from "@tanstack/react-query";

import { queryKeys } from "./keys";

// Sessions change rarely; a return to the page within this window reuses the list.
const SESSIONS_STALE_MS = 30_000;

export const sessionsQueryOptions = queryOptions({
  queryKey: queryKeys.sessions,
  queryFn: fetchSessions,
  staleTime: SESSIONS_STALE_MS,
  retry: false,
});
