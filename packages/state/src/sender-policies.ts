// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { fetchSenderPolicies } from "@huliho/core";
import { queryOptions } from "@tanstack/react-query";

import { queryKeys } from "./keys";

// A grant given on another device shows up on the next focus past this window.
const SENDER_POLICIES_STALE_MS = 30_000;

export const senderPoliciesQueryOptions = queryOptions({
  queryKey: queryKeys.senderPolicies,
  queryFn: fetchSenderPolicies,
  staleTime: SENDER_POLICIES_STALE_MS,
  retry: false,
});
