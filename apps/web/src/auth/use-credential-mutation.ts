// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { CredentialError } from "@huliho/core";
import type { CredentialFailureCode } from "@huliho/core";
import { useMutation } from "@tanstack/react-query";
import { useEffect, useState } from "react";

const COUNTDOWN_TICK_MS = 1_000;

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

function countDown(remaining: number | null): number | null {
  return remaining !== null && remaining > 1 ? remaining - 1 : null;
}

function useTick(active: boolean, onTick: () => void): void {
  useEffect(() => {
    if (!active) {
      return undefined;
    }
    const timer = setInterval(onTick, COUNTDOWN_TICK_MS);
    return () => {
      clearInterval(timer);
    };
  }, [active, onTick]);
}

// A credential check behind the sign-in limiter: a rate-limited refusal
// holds the caller for the announced seconds and ends with its countdown.
export function useCredentialMutation<TVariables>(
  mutationFn: (variables: TVariables) => Promise<void>,
  handlers: CredentialHandlers,
): CredentialMutation<TVariables> {
  const [retryRemaining, setRetryRemaining] = useState<number | null>(null);
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
        setRetryRemaining(error.retryAfterSeconds);
      }
      handlers.onFailure?.(codeOf(error));
    },
  });
  useTick(retryRemaining !== null, () => {
    setRetryRemaining(countDown);
  });
  const failure = mutation.error === null ? null : codeOf(mutation.error);
  return {
    mutate: mutation.mutate,
    pending: mutation.isPending,
    failure: failure === "rate_limited" && retryRemaining === null ? null : failure,
    retryRemaining,
  };
}
