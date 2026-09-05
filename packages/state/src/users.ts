// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { fetchUsers } from "@huliho/core";
import { queryOptions } from "@tanstack/react-query";

import { queryKeys } from "./keys";

// Users change rarely; a return to the page within this window reuses the list.
const USERS_STALE_MS = 30_000;

export const usersQueryOptions = queryOptions({
  queryKey: queryKeys.users,
  queryFn: fetchUsers,
  staleTime: USERS_STALE_MS,
  retry: false,
});
