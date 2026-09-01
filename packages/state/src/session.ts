// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { fetchSession } from "@huliho/core";
import { queryOptions } from "@tanstack/react-query";

// Route guards share one answer instead of refetching per navigation.
const SESSION_STALE_MS = 30_000;

// The one registry every query key comes from.
export const queryKeys = {
  session: ["session"],
} as const;

export const sessionQueryOptions = queryOptions({
  queryKey: queryKeys.session,
  queryFn: fetchSession,
  staleTime: SESSION_STALE_MS,
  retry: false,
});
