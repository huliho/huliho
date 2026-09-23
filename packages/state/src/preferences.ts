// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { fetchPreferences } from "@huliho/core";
import { queryOptions } from "@tanstack/react-query";

import { queryKeys } from "./keys";

// A choice made on another device shows up on the next focus past this window.
const PREFERENCES_STALE_MS = 30_000;

export const preferencesQueryOptions = queryOptions({
  queryKey: queryKeys.preferences,
  queryFn: fetchPreferences,
  staleTime: PREFERENCES_STALE_MS,
  retry: false,
});
