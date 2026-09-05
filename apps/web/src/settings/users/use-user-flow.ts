// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { UsersError, createUser, resetPassword } from "@huliho/core";
import type { CreatedUser, IssuedPassword, NewUser, UserRow, UsersFailureCode } from "@huliho/core";
import { queryKeys } from "@huliho/state";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { useSessionEnded } from "../../auth/use-session-ended";
import type { Locale } from "../../paraglide/runtime.js";

export type Flow = { kind: "create" } | { kind: "reset"; user: UserRow };

export interface Issued {
  name: string;
  secret: string;
  expiresAt: number;
  // A reset also ended the user's sessions; a creation had none to end.
  reason: "reset" | "created";
}

export interface UserFlow {
  // What the dialog is about, kept through the closing fade.
  flow: Flow | null;
  open: boolean;
  issued: Issued | null;
  pending: boolean;
  failure: UsersFailureCode | null;
  start: (flow: Flow) => void;
  confirmReset: () => void;
  submitCreate: (user: NewUser) => void;
  clearFailure: () => void;
  close: () => void;
  settle: () => void;
}

function codeOf(error: unknown): UsersFailureCode {
  return error instanceof UsersError ? error.code : "unavailable";
}

function issuedOf(
  flow: Flow | null,
  reset: IssuedPassword | undefined,
  created: CreatedUser | undefined,
): Issued | null {
  if (flow?.kind === "reset" && reset !== undefined) {
    return {
      name: flow.user.name,
      secret: reset.oneTimePassword,
      expiresAt: reset.expiresAt,
      reason: "reset",
    };
  }
  if (flow?.kind === "create" && created !== undefined) {
    return {
      name: created.user.name,
      secret: created.oneTimePassword,
      expiresAt: created.expiresAt,
      reason: "created",
    };
  }
  return null;
}

// Owns the reset and create requests behind the users dialog. The
// one-time password lives in the mutation result until the dialog has
// closed, then both mutations forget it.
export function useUserFlow(locale: Locale): UserFlow {
  const queryClient = useQueryClient();
  const sessionEnded = useSessionEnded(locale);
  const [flow, setFlow] = useState<Flow | null>(null);
  const [open, setOpen] = useState(false);
  const onError = (error: unknown): void => {
    if (codeOf(error) === "unauthenticated") {
      sessionEnded();
    }
  };
  // Both answers hold a one-time password. Without a cache window the
  // reset drops the mutation at once, so the secret never outlives the dialog.
  const reset = useMutation({ mutationFn: resetPassword, onError, gcTime: 0 });
  const create = useMutation({
    mutationFn: createUser,
    onError,
    gcTime: 0,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.users }),
  });
  const error = reset.error ?? create.error;
  const clearFailure = (): void => {
    reset.reset();
    create.reset();
  };
  return {
    flow,
    open,
    issued: issuedOf(flow, reset.data, create.data),
    pending: reset.isPending || create.isPending,
    failure: error === null ? null : codeOf(error),
    // A reopen inside the closing fade must not show the previous answer.
    start: (next) => {
      clearFailure();
      setFlow(next);
      setOpen(true);
    },
    confirmReset: () => {
      if (flow?.kind === "reset") {
        reset.mutate(flow.user.id);
      }
    },
    submitCreate: (user) => {
      create.mutate(user);
    },
    clearFailure,
    close: () => {
      setOpen(false);
    },
    settle: () => {
      setFlow(null);
      clearFailure();
    },
  };
}
