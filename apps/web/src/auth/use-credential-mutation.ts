// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { CredentialError } from "@huliho/core";
import type { CredentialFailureCode } from "@huliho/core";
import { useMutation } from "@tanstack/react-query";

import { useRetryCountdown } from "./use-retry-countdown";

export interface CredentialMutation<TVariables> {
  mutate: (variables: TVariables) => void;
  pending: boolean;
  failure: CredentialFailureCode | null;
  // Seconds until the limiter lets the next attempt through; null once it does.
  retryRemaining: number | null;
}

interface CredentialHandlers {
  onSuccess: () => Promise<void> | void;
  onFailure?: ((failure: CredentialFailureCode) => void) | undefined;
}

function codeOf(error: unknown): CredentialFailureCode {
  return error instanceof CredentialError ? error.code : "unavailable";
}

// A credential check behind the sign-in limiter: a rate-limited refusal
// holds the caller for the announced seconds and ends with its countdown.
export function useCredentialMutation<TVariables>(
  mutationFn: (variables: TVariables) => Promise<void>,
  handlers: CredentialHandlers,
): CredentialMutation<TVariables> {
  const countdown = useRetryCountdown();
  const mutation = useMutation({
    mutationFn,
    // The reset detaches the mutation and no cache time removes it at once,
    // so no password outlives its request.
    gcTime: 0,
    onSuccess: async () => {
      await handlers.onSuccess();
      mutation.reset();
    },
    onError: (error) => {
      if (error instanceof CredentialError && error.code === "rate_limited") {
        countdown.start(error.retryAfterSeconds);
      }
      handlers.onFailure?.(codeOf(error));
    },
  });
  const failure = mutation.error === null ? null : codeOf(mutation.error);
  return {
    mutate: mutation.mutate,
    pending: mutation.isPending,
    failure: failure === "rate_limited" && countdown.retryRemaining === null ? null : failure,
    retryRemaining: countdown.retryRemaining,
  };
}
