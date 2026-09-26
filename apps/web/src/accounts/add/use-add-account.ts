// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import {
  AccountsError,
  addAccount,
  discoverServer,
  endConsent,
  replaceCredential,
  startConsent,
} from "@huliho/core";
import type {
  AccountRow,
  AccountTarget,
  AccountsFailureCode,
  ConsentInput,
  ConsentOutcome,
  Credential,
  Provider,
  SignInProvider,
} from "@huliho/core";
import { consentQueryOptions } from "@huliho/state";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useEffect, useReducer, useRef } from "react";
import type { RefObject } from "react";

import { useRetryCountdown } from "../../auth/use-retry-countdown";
import type { RetryCountdown } from "../../auth/use-retry-countdown";
import { useSessionEnded } from "../../auth/use-session-ended";
import type { Locale } from "../../paraglide/runtime.js";
import { openConsentWindow } from "./consent-window";
import { initialState, reduce } from "./flow";
import type { Action, ConsentStep, FlowState, Step } from "./flow";
import { mailProviderOf, providerName } from "./presets";

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
  // Opens the provider's window from the click and starts the consent.
  startConsent: (signIn: SignInProvider) => void;
  // Opens the window from a fresh click when the browser kept it closed.
  openConsentWindow: () => void;
  cancelConsent: () => void;
  usePassword: () => void;
}

type RequestActions = Pick<AddAccountFlow, "detect" | "connect" | "reconnect">;

interface ReplaceInput {
  id: string;
  credential: Credential;
}

// The window a consent runs in and where it should go; a handle is
// nothing a reducer can hold, so both live beside the machine.
interface HeldWindow {
  handle: WindowProxy | null;
  url: string | null;
  // True from the click until the consent ends, so a start answer that
  // lands after Cancel finds nothing to send.
  waiting: boolean;
}

// Everything a request's answer reaches for.
interface Machine {
  dispatch: (action: Action) => void;
  held: RefObject<HeldWindow>;
  // Told the row that connected, by id and name.
  onConnected: (id: string, name: string) => void;
  sessionEnded: () => void;
  countdown: RetryCountdown;
}

type SettledOutcome = Exclude<ConsentOutcome, { status: "pending" }>;

// What the poll said, once it said something to act on.
type Answer = SettledOutcome | { status: "failed"; failure: AccountsFailureCode };

type Requests = ReturnType<typeof useRequests>;

function codeOf(error: unknown): AccountsFailureCode {
  return error instanceof AccountsError ? error.code : "unavailable";
}

// How often a refused end of a consent is sent again: once, so a dropped
// request still closes the consent the server holds open.
const END_RETRIES = 1;

// Discovery stores nothing, so a request the network dropped is safe to
// send again; the retry waits for the browser to be back online.
function retryDiscovery(count: number, error: unknown): boolean {
  return count === 0 && codeOf(error) === "unavailable" && !navigator.onLine;
}

// Lets go of the window. A blank one still closes; one at the provider
// went there without an opener, so it is the user's to close from then on.
function release(held: HeldWindow): void {
  held.handle?.close();
  held.handle = null;
  held.url = null;
  held.waiting = false;
}

// Opens the window into the held slot; true when the browser let it.
function holdWindow(held: HeldWindow, url: string | null): boolean {
  held.handle = openConsentWindow(url);
  held.waiting = true;
  return held.handle !== null;
}

// The start answered: the window goes to the provider and the URL stays
// for a window the browser kept closed. False once the consent ended.
function sendWindow(held: HeldWindow, url: string): boolean {
  if (!held.waiting) {
    return false;
  }
  held.url = url;
  held.handle?.location.assign(url);
  return true;
}

// The toast's name: the row's own on a reconnect, the preset's for a new
// account, which is how the server names it.
function consentName(step: ConsentStep, address: string): string {
  return step.from.name === "reconnect"
    ? step.from.account.name
    : providerName(mailProviderOf(step.signIn), address);
}

// The start request: the preset, the address and the row on a reconnect.
function consentInput(step: Step, signIn: SignInProvider, address: string): ConsentInput {
  const provider = mailProviderOf(signIn);
  const origin = step.name === "consentDenied" ? step.from : step;
  return origin.name === "reconnect"
    ? { provider, address, accountId: origin.account.id }
    : { provider, address };
}

// A refusal lands in the machine; a session that ended lands on sign-in.
function refuse(machine: Machine, error: unknown): void {
  const failure = codeOf(error);
  if (failure === "unauthenticated") {
    machine.sessionEnded();
    return;
  }
  if (error instanceof AccountsError && failure === "rate_limited") {
    machine.countdown.start(error.retryAfterSeconds);
  }
  machine.dispatch({ type: "failed", failure });
}

// The four requests; the variables of a connect hold the credential, so
// no cache time keeps them once the card is gone.
function useRequests(machine: Machine) {
  const { dispatch, held } = machine;
  const failed = (error: unknown): void => {
    refuse(machine, error);
  };
  const connected = (row: AccountRow): void => {
    dispatch({ type: "connected" });
    machine.onConnected(row.id, row.name);
  };
  const discovery = useMutation({
    mutationFn: discoverServer,
    retry: retryDiscovery,
    onSuccess: (found) => {
      dispatch(found === null ? { type: "nothingFound" } : { type: "detected", found });
    },
    onError: failed,
  });
  const add = useMutation({
    mutationFn: addAccount,
    gcTime: 0,
    onSuccess: connected,
    onError: failed,
  });
  const replace = useMutation({
    mutationFn: (input: ReplaceInput) => replaceCredential(input.id, input.credential),
    gcTime: 0,
    onSuccess: connected,
    onError: failed,
  });
  // Cancel ends the consent on the server, so a window still at the
  // provider lands nothing. A refused end is tried once more; only a
  // session that ended is worth a word.
  const end = useMutation({
    mutationFn: endConsent,
    gcTime: 0,
    retry: (count, error) => count < END_RETRIES && codeOf(error) !== "unauthenticated",
    onError: (error) => {
      if (codeOf(error) === "unauthenticated") {
        machine.sessionEnded();
      }
    },
  });
  const start = useMutation({
    mutationFn: startConsent,
    gcTime: 0,
    onSuccess: (started) => {
      if (sendWindow(held.current, started.url)) {
        dispatch({ type: "consentStarted", id: started.state });
      } else {
        end.mutate(started.state);
      }
    },
    onError: (error) => {
      release(held.current);
      failed(error);
    },
  });
  return { discovery, add, replace, start, end };
}

// An error outranks the data a failed refetch leaves in place; pending
// is nothing to act on yet.
function answerOf(outcome: ConsentOutcome | null, error: unknown): Answer | null {
  if (error !== null) {
    return { status: "failed", failure: codeOf(error) };
  }
  return outcome === null || outcome.status === "pending" ? null : outcome;
}

// What a settled poll does to the machine. A poll that failed for any
// reason but the session ending keeps going.
function settle(machine: Machine, name: string, answer: Answer): void {
  const { dispatch } = machine;
  const held = machine.held.current;
  if (answer.status === "failed") {
    if (answer.failure === "unauthenticated") {
      release(held);
      machine.sessionEnded();
    }
    return;
  }
  release(held);
  if (answer.status === "done") {
    dispatch({ type: "connected" });
    machine.onConnected(answer.accountId, name);
    return;
  }
  dispatch({ type: "consentRefused", cause: answer.status === "gone" ? "gone" : answer.cause });
}

// The consent's poll, settled into the machine once per answer: a ref
// remembers the answer handled, so a repeated effect run is inert.
function useConsentPoll(consent: ConsentStep | null, address: string, machine: Machine): void {
  const poll = useQuery(consentQueryOptions(consent?.id ?? null));
  const outcome = poll.data ?? null;
  const { error } = poll;
  const handled = useRef<unknown>(null);
  useEffect(() => {
    const seen = error ?? outcome;
    const answer = answerOf(outcome, error);
    if (consent === null || answer === null || seen === handled.current) {
      return;
    }
    handled.current = seen;
    settle(machine, consentName(consent, address), answer);
  }, [consent, address, outcome, error, machine]);
}

// The requests behind the first screens: discovery, the connect and the
// reconnect by credential.
function requestActions(
  state: FlowState,
  account: AccountRow | null,
  requests: Requests,
  dispatch: (action: Action) => void,
): RequestActions {
  return {
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

// Owns the step machine, the requests behind it and the consent window.
export function useAddAccount(
  locale: Locale,
  account: AccountRow | null,
  onConnected: (id: string, name: string) => void,
): AddAccountFlow {
  const [state, dispatch] = useReducer(reduce, account, initialState);
  const countdown = useRetryCountdown();
  const sessionEnded = useSessionEnded(locale);
  const held = useRef<HeldWindow>({ handle: null, url: null, waiting: false });
  const machine: Machine = { dispatch, held, onConnected, sessionEnded, countdown };
  const requests = useRequests(machine);
  const { step } = state;
  useConsentPoll(step.name === "consent" ? step : null, state.address, machine);
  // Leaving the card closes a window that has not left yet.
  useEffect(
    () => () => {
      release(held.current);
    },
    [],
  );
  return {
    state,
    retryRemaining: countdown.retryRemaining,
    dispatch,
    ...requestActions(state, account, requests, dispatch),
    startConsent: (signIn) => {
      const opened = holdWindow(held.current, null);
      dispatch({ type: "startConsent", signIn, opened });
      requests.start.mutate(consentInput(step, signIn, state.address));
    },
    openConsentWindow: () => {
      const { url } = held.current;
      if (url !== null && holdWindow(held.current, url)) {
        dispatch({ type: "consentOpened" });
      }
    },
    cancelConsent: () => {
      release(held.current);
      if (step.name === "consent" && step.id !== null) {
        requests.end.mutate(step.id);
      }
      dispatch({ type: "cancelConsent" });
    },
    usePassword: () => {
      dispatch({ type: "usePassword" });
      if (step.name === "consentDenied" && step.from.name === "typing") {
        requests.discovery.mutate(state.address);
      }
    },
  };
}
