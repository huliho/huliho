// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { AccountsError, retryAccount } from "@huliho/core";
import type { AccountList, AccountRow, AccountsFailureCode } from "@huliho/core";
import { queryKeys } from "@huliho/state";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { QueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { useSessionEnded } from "../../auth/use-session-ended";
import type { Locale } from "../../paraglide/runtime.js";

// What the last retry of a row did, as the row shows it; failed covers
// every retry that settled nothing, whatever refused it along the way.
export type RetryOutcome = "pending" | "resumed" | "stillStopped" | "failed";

export type RetryOutcomes = Partial<Record<string, RetryOutcome>>;

export interface AccountRetry {
  outcomes: RetryOutcomes;
  retry: (id: string) => void;
  // Drops what a row's last retry said, so a row that returns says nothing stale.
  forget: (id: string) => void;
}

function codeOf(error: unknown): AccountsFailureCode {
  return error instanceof AccountsError ? error.code : "unavailable";
}

function without(outcomes: RetryOutcomes, id: string): RetryOutcomes {
  return Object.fromEntries(Object.entries(outcomes).filter(([key]) => key !== id));
}

// The list with one row changed; an evicted cache stays evicted.
function patchRow(
  queryClient: QueryClient,
  id: string,
  patch: (row: AccountRow) => AccountRow,
): void {
  queryClient.setQueryData<AccountList>(queryKeys.accounts, (list) =>
    list === undefined
      ? undefined
      : { ...list, accounts: list.accounts.map((row) => (row.id === id ? patch(row) : row)) },
  );
}

// One mutation runs every retry; the outcome per row lives beside it, so
// two rows can retry at once and each says its own.
export function useRetryAccount(locale: Locale): AccountRetry {
  const queryClient = useQueryClient();
  const sessionEnded = useSessionEnded(locale);
  const [outcomes, setOutcomes] = useState<RetryOutcomes>({});
  const mark = (id: string, outcome: RetryOutcome): void => {
    setOutcomes((current) => ({ ...current, [id]: outcome }));
  };
  const mutation = useMutation({
    mutationFn: retryAccount,
    onMutate: (id) => {
      mark(id, "pending");
    },
    onSuccess: (result, id) => {
      if (result.status === "resumed") {
        const { account } = result;
        patchRow(queryClient, id, () => account);
        mark(id, "resumed");
        return;
      }
      const { cause } = result;
      patchRow(queryClient, id, (row) => ({ ...row, stoppedCause: cause }));
      mark(id, "stillStopped");
    },
    onError: (error, id) => {
      const code = codeOf(error);
      if (code === "unauthenticated") {
        sessionEnded();
        return;
      }
      mark(id, "failed");
      // A row that is gone leaves the list with the next answer.
      if (code === "not_found") {
        void queryClient.invalidateQueries({ queryKey: queryKeys.accounts });
      }
    },
  });
  return {
    outcomes,
    retry: (id) => {
      mutation.mutate(id);
    },
    forget: (id) => {
      setOutcomes((current) => without(current, id));
    },
  };
}
