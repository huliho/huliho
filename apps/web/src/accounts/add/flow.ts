// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type {
  AccountRow,
  AccountsFailureCode,
  ConsentDeniedCause,
  FoundServer,
  SignInProvider,
} from "@huliho/core";

// The steps with a credential field; a refusal returns to the one it left.
export type FieldStep =
  | { name: "found"; found: FoundServer }
  | { name: "manual" }
  | { name: "reconnect"; account: AccountRow };

// Where a consent started; Cancel and a refused start return there.
export type ConsentOrigin =
  | { name: "typing" }
  | { name: "found"; found: FoundServer }
  | { name: "reconnect"; account: AccountRow };

// Why a consent ended without an account; gone is one that ran out.
export type ConsentRefusal = ConsentDeniedCause | "gone";

export interface ConsentStep {
  name: "consent";
  signIn: SignInProvider;
  from: ConsentOrigin;
  // What the poll asks for; null until the start request answers.
  id: string | null;
  // Whether the click could open the window.
  opened: boolean;
}

export interface ConsentDeniedStep {
  name: "consentDenied";
  signIn: SignInProvider;
  from: ConsentOrigin;
  cause: ConsentRefusal;
}

export type Step =
  | FieldStep
  | ConsentStep
  | ConsentDeniedStep
  | { name: "typing" }
  | { name: "detecting" }
  | { name: "confirmHost"; found: FoundServer }
  | { name: "notFound" }
  | { name: "connecting"; from: FieldStep; host: string }
  | { name: "insecure"; from: FieldStep }
  | { name: "connected" };

type SettledStep = Extract<
  Step,
  { name: "found" | "confirmHost" | "manual" | "reconnect" | "connected" }
>;

export interface FlowState {
  step: Step;
  address: string;
  // The refusal the current step shows; null once a field changes.
  failure: AccountsFailureCode | null;
}

export type Action =
  | { type: "typed"; address: string }
  | { type: "detect" }
  | { type: "detected"; found: FoundServer }
  | { type: "nothingFound" }
  | { type: "change" }
  | { type: "continue" }
  | { type: "differentServer" }
  | { type: "enterDetails" }
  | { type: "connect"; host: string }
  | { type: "failed"; failure: AccountsFailureCode }
  | { type: "back" }
  | { type: "edited" }
  | { type: "connected" }
  | { type: "startConsent"; signIn: SignInProvider; opened: boolean }
  | { type: "consentStarted"; id: string }
  | { type: "consentOpened" }
  | { type: "consentRefused"; cause: ConsentRefusal }
  | { type: "cancelConsent" }
  | { type: "usePassword" };

// A fresh add starts by typing; a reconnect opens on the credential step
// with the row's address fixed.
export function initialState(account: AccountRow | null): FlowState {
  if (account === null) {
    return { step: { name: "typing" }, address: "", failure: null };
  }
  return { step: { name: "reconnect", account }, address: account.address, failure: null };
}

function at(state: FlowState, step: Step): FlowState {
  return { ...state, step, failure: null };
}

function reduceAddress(state: FlowState, action: Action): FlowState {
  switch (action.type) {
    case "typed":
      return { ...state, address: action.address, failure: null };
    case "detect":
      return at(state, { name: "detecting" });
    case "enterDetails":
      return state.step.name === "notFound" ? at(state, { name: "manual" }) : state;
    case "failed":
      return { ...state, failure: action.failure };
    default:
      return state;
  }
}

function reduceDetecting(state: FlowState, action: Action): FlowState {
  switch (action.type) {
    case "detected":
      return at(state, { name: "found", found: action.found });
    case "nothingFound":
      return at(state, { name: "notFound" });
    case "failed":
      return { ...state, step: { name: "typing" }, failure: action.failure };
    default:
      return state;
  }
}

function reduceFound(state: FlowState, found: FoundServer, action: Action): FlowState {
  switch (action.type) {
    case "change":
      return at(state, { name: "typing" });
    case "continue":
      return at(state, { name: "confirmHost", found });
    default:
      return state;
  }
}

function reduceConfirmHost(state: FlowState, found: FoundServer, action: Action): FlowState {
  switch (action.type) {
    case "differentServer":
      return at(state, { name: "manual" });
    case "connect":
      return at(state, { name: "connecting", from: { name: "found", found }, host: action.host });
    default:
      return state;
  }
}

function reduceConnecting(state: FlowState, from: FieldStep, action: Action): FlowState {
  switch (action.type) {
    case "connected":
      return at(state, { name: "connected" });
    case "failed":
      return action.failure === "upstream_insecure"
        ? at(state, { name: "insecure", from })
        : { ...state, step: from, failure: action.failure };
    default:
      return state;
  }
}

// The steps a consent can start from, each its own origin to return to.
function consentOrigin(step: Step): ConsentOrigin | null {
  switch (step.name) {
    case "typing":
      return { name: "typing" };
    case "found":
      return { name: "found", found: step.found };
    case "reconnect":
      return { name: "reconnect", account: step.account };
    case "consentDenied":
      return step.from;
    default:
      return null;
  }
}

function reduceConsent(state: FlowState, step: ConsentStep, action: Action): FlowState {
  switch (action.type) {
    case "consentStarted":
      return at(state, { ...step, id: action.id });
    case "consentOpened":
      return at(state, { ...step, opened: true });
    case "consentRefused":
      return at(state, {
        name: "consentDenied",
        signIn: step.signIn,
        from: step.from,
        cause: action.cause,
      });
    case "cancelConsent":
      return at(state, step.from);
    case "failed":
      return { ...state, step: step.from, failure: action.failure };
    case "connected":
      return at(state, { name: "connected" });
    default:
      return state;
  }
}

// The password route of a denied consent leads back to the found step;
// a consent that began on the first screen runs discovery instead.
function reduceConsentDenied(state: FlowState, step: ConsentDeniedStep, action: Action): FlowState {
  if (action.type !== "usePassword" || step.from.name === "reconnect") {
    return state;
  }
  return at(state, step.from.name === "typing" ? { name: "detecting" } : step.from);
}

// The steps that wait for the user with no request running.
function reduceSettled(state: FlowState, step: SettledStep, action: Action): FlowState {
  switch (step.name) {
    case "found":
      return reduceFound(state, step.found, action);
    case "confirmHost":
      return reduceConfirmHost(state, step.found, action);
    case "manual":
    case "reconnect":
      return action.type === "connect"
        ? at(state, { name: "connecting", from: step, host: action.host })
        : state;
    default:
      return state;
  }
}

function reduceStep(state: FlowState, action: Action): FlowState {
  const { step } = state;
  switch (step.name) {
    case "typing":
    case "notFound":
      return reduceAddress(state, action);
    case "detecting":
      return reduceDetecting(state, action);
    case "connecting":
      return reduceConnecting(state, step.from, action);
    case "insecure":
      return action.type === "back" ? at(state, step.from) : state;
    case "consent":
      return reduceConsent(state, step, action);
    case "consentDenied":
      return reduceConsentDenied(state, step, action);
    default:
      return reduceSettled(state, step, action);
  }
}

export function reduce(state: FlowState, action: Action): FlowState {
  if (action.type === "edited") {
    return state.failure === null ? state : { ...state, failure: null };
  }
  if (action.type === "startConsent") {
    const from = consentOrigin(state.step);
    return from === null
      ? state
      : at(state, {
          name: "consent",
          signIn: action.signIn,
          from,
          id: null,
          opened: action.opened,
        });
  }
  return reduceStep(state, action);
}
