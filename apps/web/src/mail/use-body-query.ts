// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { BodyDetail, MailCache } from "@huliho/core";
import { bodyQueryOptions, queryKeys } from "@huliho/state";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { UseQueryResult } from "@tanstack/react-query";

import { toastManager } from "../design-system/toast";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";

export interface BodyAsk {
  // The body as first asked; null for an email the server does not have.
  query: UseQueryResult<BodyDetail | null>;
  // Asks the body once more at the large cap; the answer takes the
  // query's place.
  showWhole: () => void;
  wholePending: boolean;
}

// The body of one email through the cache, with the second ask a cut
// body offers. A second ask that fails leaves the first answer standing
// and says so; like the queries it runs whatever the network does, so
// offline it fails at once.
export function useBodyQuery(
  cache: MailCache,
  accountId: string,
  emailId: string,
  locale: Locale,
): BodyAsk {
  const queryClient = useQueryClient();
  const query = useQuery(bodyQueryOptions(cache, accountId, emailId));
  const whole = useMutation({
    networkMode: "always",
    mutationFn: () => cache.body(accountId, emailId, { large: true }),
    onSuccess: (detail) => {
      queryClient.setQueryData(queryKeys.body(accountId, emailId), detail);
    },
    onError: () => {
      toastManager.add({ description: m.body_error({}, { locale }) });
    },
  });
  return {
    query,
    showWhole: () => {
      whole.mutate();
    },
    wholePending: whole.isPending,
  };
}
