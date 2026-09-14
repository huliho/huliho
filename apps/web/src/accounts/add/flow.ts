// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow, AccountsFailureCode, FoundServer } from "@huliho/core";

// The steps with a credential field; a refusal returns to the one it left.
export type FieldStep =
  | { name: "found"; found: FoundServer }
  | { name: "manual" }
  | { name: "reconnect"; account: AccountRow };

export type Step =
  | FieldStep
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
  | { type: "connected" };

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

export function reduce(state: FlowState, action: Action): FlowState {
  if (action.type === "edited") {
    return state.failure === null ? state : { ...state, failure: null };
  }
  const { step } = state;
  if (step.name === "typing" || step.name === "notFound") {
    return reduceAddress(state, action);
  }
  if (step.name === "detecting") {
    return reduceDetecting(state, action);
  }
  if (step.name === "connecting") {
    return reduceConnecting(state, step.from, action);
  }
  if (step.name === "insecure") {
    return action.type === "back" ? at(state, step.from) : state;
  }
  return reduceSettled(state, step, action);
}
