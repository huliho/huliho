// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { AccountsError, addAccount, discoverServer, replaceCredential } from "@huliho/core";
import type {
  AccountRow,
  AccountTarget,
  AccountsFailureCode,
  Credential,
  FoundServer,
  Provider,
} from "@huliho/core";
import { useMutation } from "@tanstack/react-query";
import { useReducer } from "react";

import { useRetryCountdown } from "../../auth/use-retry-countdown";
import { useSessionEnded } from "../../auth/use-session-ended";
import type { Locale } from "../../paraglide/runtime.js";
import { initialState, reduce } from "./flow";
import type { Action, FlowState } from "./flow";

// What a connect sends: the target the user saw with the credential, once.
export interface ConnectInput {
  provider: Provider;
  target: AccountTarget;
  credential: Credential;
  // What the connecting sentence names.
  host: string;
}

export interface AddAccountFlow {
  state: FlowState;
  // Seconds until the limiter lets the next attempt through; null once it does.
  retryRemaining: number | null;
  dispatch: (action: Action) => void;
  detect: () => void;
  connect: (input: ConnectInput) => void;
  reconnect: (credential: Credential) => void;
}

interface Outcomes {
  found: (found: FoundServer | null) => void;
  connected: (row: AccountRow) => void;
  failed: (error: unknown) => void;
}

interface ReplaceInput {
  id: string;
  credential: Credential;
}

function codeOf(error: unknown): AccountsFailureCode {
  return error instanceof AccountsError ? error.code : "unavailable";
}

// Discovery stores nothing, so a request the network dropped is safe to
// send again; the retry waits for the browser to be back online.
function retryDiscovery(count: number, error: unknown): boolean {
  return count === 0 && codeOf(error) === "unavailable" && !navigator.onLine;
}

// The three requests; the variables of a connect hold the credential, so
// no cache time keeps them once the card is gone.
function useRequests(outcomes: Outcomes) {
  const discovery = useMutation({
    mutationFn: discoverServer,
    retry: retryDiscovery,
    onSuccess: outcomes.found,
    onError: outcomes.failed,
  });
  const add = useMutation({
    mutationFn: addAccount,
    gcTime: 0,
    onSuccess: outcomes.connected,
    onError: outcomes.failed,
  });
  const replace = useMutation({
    mutationFn: (input: ReplaceInput) => replaceCredential(input.id, input.credential),
    gcTime: 0,
    onSuccess: outcomes.connected,
    onError: outcomes.failed,
  });
  return { discovery, add, replace };
}

// Owns the step machine and the requests behind it. A refusal lands in
// the machine; a session that ended lands on sign-in.
export function useAddAccount(
  locale: Locale,
  account: AccountRow | null,
  onConnected: (row: AccountRow) => void,
): AddAccountFlow {
  const [state, dispatch] = useReducer(reduce, account, initialState);
  const countdown = useRetryCountdown();
  const sessionEnded = useSessionEnded(locale);
  const requests = useRequests({
    found: (found) => {
      dispatch(found === null ? { type: "nothingFound" } : { type: "detected", found });
    },
    connected: (row) => {
      dispatch({ type: "connected" });
      onConnected(row);
    },
    failed: (error) => {
      const failure = codeOf(error);
      if (failure === "unauthenticated") {
        sessionEnded();
        return;
      }
      if (error instanceof AccountsError && failure === "rate_limited") {
        countdown.start(error.retryAfterSeconds);
      }
      dispatch({ type: "failed", failure });
    },
  });
  return {
    state,
    retryRemaining: countdown.retryRemaining,
    dispatch,
    detect: () => {
      dispatch({ type: "detect" });
      requests.discovery.mutate(state.address);
    },
    connect: (input) => {
      dispatch({ type: "connect", host: input.host });
      requests.add.mutate({
        address: state.address,
        provider: input.provider,
        target: input.target,
        credential: input.credential,
      });
    },
    reconnect: (credential) => {
      if (account === null) {
        return;
      }
      dispatch({ type: "connect", host: account.name });
      requests.replace.mutate({ id: account.id, credential });
    },
  };
}
