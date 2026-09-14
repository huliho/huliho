// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow, FoundServer } from "@huliho/core";
import { expect, test } from "vitest";

import { initialState, reduce } from "./flow";
import type { Action, FlowState, Step } from "./flow";

const ADDRESS = "sanne@fastmail.com";
const FOUND: FoundServer = {
  provider: "fastmail",
  kind: "jmap",
  target: { kind: "jmap", sessionUrl: "https://api.fastmail.com/jmap/session" },
  credentialKind: "apiToken",
  host: "api.fastmail.com",
  oauthAvailable: false,
};
const ROW: AccountRow = {
  id: "acc-1",
  address: ADDRESS,
  name: "Fastmail",
  provider: "fastmail",
  kind: "jmap",
  authMethod: "bearer",
  stoppedCause: "credentials",
  stoppedAt: 1_778_750_400_000,
  createdAt: 1_778_664_000_000,
};

function at(step: Step, failure: FlowState["failure"] = null): FlowState {
  return { step, address: ADDRESS, failure };
}

function run(state: FlowState, ...actions: Action[]): FlowState {
  return actions.reduce(reduce, state);
}

test("an add starts by typing; a reconnect opens on the credential step with the address fixed", () => {
  expect(initialState(null)).toEqual({ step: { name: "typing" }, address: "", failure: null });
  expect(initialState(ROW)).toEqual(at({ name: "reconnect", account: ROW }));
});

test("the discovered route: typing, detecting, found, confirm, connecting, connected", () => {
  const typed = run(initialState(null), { type: "typed", address: ADDRESS });
  expect(typed).toEqual(at({ name: "typing" }));
  const detecting = run(typed, { type: "detect" });
  expect(detecting).toEqual(at({ name: "detecting" }));
  const found = run(detecting, { type: "detected", found: FOUND });
  expect(found).toEqual(at({ name: "found", found: FOUND }));
  const confirm = run(found, { type: "continue" });
  expect(confirm).toEqual(at({ name: "confirmHost", found: FOUND }));
  const connecting = run(confirm, { type: "connect", host: FOUND.host });
  expect(connecting).toEqual(
    at({ name: "connecting", from: { name: "found", found: FOUND }, host: FOUND.host }),
  );
  expect(run(connecting, { type: "connected" })).toEqual(at({ name: "connected" }));
});

test("nothing found offers manual entry, which connects from its own fields", () => {
  const notFound = run(at({ name: "detecting" }), { type: "nothingFound" });
  expect(notFound).toEqual(at({ name: "notFound" }));
  expect(run(notFound, { type: "detect" })).toEqual(at({ name: "detecting" }));
  expect(run(notFound, { type: "typed", address: "sanne@dekker-mail.nl" }).address).toBe(
    "sanne@dekker-mail.nl",
  );
  const manual = run(notFound, { type: "enterDetails" });
  expect(manual).toEqual(at({ name: "manual" }));
  expect(run(manual, { type: "connect", host: "mail.dekker-mail.nl" })).toEqual(
    at({ name: "connecting", from: { name: "manual" }, host: "mail.dekker-mail.nl" }),
  );
});

test("a different server from the confirm step goes to manual", () => {
  expect(run(at({ name: "confirmHost", found: FOUND }), { type: "differentServer" })).toEqual(
    at({ name: "manual" }),
  );
});

test("change from found returns to typing with the address kept", () => {
  expect(run(at({ name: "found", found: FOUND }), { type: "change" })).toEqual(
    at({ name: "typing" }),
  );
});

test("a refusal returns to the field step it left; an insecure server gets its own screen", () => {
  const fromFound: Step = { name: "connecting", from: { name: "found", found: FOUND }, host: "h" };
  expect(run(at(fromFound), { type: "failed", failure: "upstream_credentials" })).toEqual(
    at({ name: "found", found: FOUND }, "upstream_credentials"),
  );
  const fromManual: Step = { name: "connecting", from: { name: "manual" }, host: "h" };
  expect(run(at(fromManual), { type: "failed", failure: "upstream_unreachable" })).toEqual(
    at({ name: "manual" }, "upstream_unreachable"),
  );
  const insecure = run(at(fromManual), { type: "failed", failure: "upstream_insecure" });
  expect(insecure).toEqual(at({ name: "insecure", from: { name: "manual" } }));
  expect(run(insecure, { type: "back" })).toEqual(at({ name: "manual" }));
});

test("a refusal during discovery lands on typing", () => {
  expect(run(at({ name: "detecting" }), { type: "failed", failure: "rate_limited" })).toEqual(
    at({ name: "typing" }, "rate_limited"),
  );
  expect(run(at({ name: "typing" }), { type: "failed", failure: "invalid_request" })).toEqual(
    at({ name: "typing" }, "invalid_request"),
  );
});

test("a reconnect connects from its step and returns to it after a refusal", () => {
  const step: Step = { name: "reconnect", account: ROW };
  const connecting = run(at(step), { type: "connect", host: ROW.name });
  expect(connecting).toEqual(at({ name: "connecting", from: step, host: ROW.name }));
  expect(run(connecting, { type: "failed", failure: "upstream_credentials" })).toEqual(
    at(step, "upstream_credentials"),
  );
});

test("edited clears the refusal and touches nothing else", () => {
  const refused = at({ name: "found", found: FOUND }, "upstream_credentials");
  expect(run(refused, { type: "edited" })).toEqual(at({ name: "found", found: FOUND }));
  const clean = at({ name: "typing" });
  expect(run(clean, { type: "edited" })).toBe(clean);
});

test("an action that does not fit the step leaves the state alone", () => {
  const cases: [FlowState, Action][] = [
    [at({ name: "typing" }), { type: "connected" }],
    [at({ name: "typing" }), { type: "connect", host: "h" }],
    [at({ name: "found", found: FOUND }), { type: "detected", found: FOUND }],
    [at({ name: "found", found: FOUND }), { type: "typed", address: "x@example.test" }],
    [at({ name: "manual" }), { type: "back" }],
    [at({ name: "typing" }), { type: "enterDetails" }],
    [at({ name: "connected" }), { type: "detect" }],
  ];
  for (const [state, action] of cases) {
    expect(reduce(state, action)).toBe(state);
  }
});
