// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow, FoundServer } from "@huliho/core";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import { AddAccountCard } from "./add-account-card";
import type { FlowState, Step } from "./flow";
import type { AddAccountFlow, ConnectInput } from "./use-add-account";

const ADDRESS = "sanne@fastmail.com";
const FASTMAIL: FoundServer = {
  provider: "fastmail",
  kind: "jmap",
  target: { kind: "jmap", sessionUrl: "https://api.fastmail.com/jmap/session" },
  credentialKind: "apiToken",
  host: "api.fastmail.com",
  oauthAvailable: false,
};
const GENERIC: FoundServer = {
  provider: "generic",
  kind: "imap",
  target: {
    kind: "imap",
    username: "sanne@dekker-mail.nl",
    imap: { host: "imap.dekker-mail.nl", port: 993, tls: "implicit" },
    smtp: { host: "smtp.dekker-mail.nl", port: 465, tls: "implicit" },
  },
  credentialKind: "password",
  host: "imap.dekker-mail.nl",
  oauthAvailable: false,
};
const MICROSOFT: FoundServer = {
  ...GENERIC,
  provider: "microsoft",
  credentialKind: "oauth",
  host: "outlook.office365.com",
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

interface Rendered {
  flow: AddAccountFlow;
  dispatch: Mock<AddAccountFlow["dispatch"]>;
  connect: Mock<(input: ConnectInput) => void>;
  reconnect: Mock<AddAccountFlow["reconnect"]>;
  // Renders the same card again with another state, as a refusal would.
  update: (state: FlowState) => void;
}

afterEach(cleanup);

function renderCard(state: FlowState, online = true): Rendered {
  const dispatch = vi.fn<AddAccountFlow["dispatch"]>();
  const connect = vi.fn<(input: ConnectInput) => void>();
  const reconnect = vi.fn<AddAccountFlow["reconnect"]>();
  const flow: AddAccountFlow = {
    state,
    retryRemaining: null,
    dispatch,
    detect: vi.fn<() => void>(),
    connect,
    reconnect,
  };
  const { rerender } = render(<AddAccountCard locale="en" flow={flow} online={online} />);
  const update = (next: FlowState): void => {
    rerender(<AddAccountCard locale="en" flow={{ ...flow, state: next }} online={online} />);
  };
  return { flow, dispatch, connect, reconnect, update };
}

function at(step: Step, failure: FlowState["failure"] = null): FlowState {
  return { step, address: ADDRESS, failure };
}

function fill(label: string, value: string): void {
  fireEvent.change(screen.getByLabelText(label), { target: { value } });
}

test("the offline sentence shows on any step as a status", () => {
  renderCard(at({ name: "found", found: FASTMAIL }), false);
  expect(screen.getByRole("status").textContent).toContain("You’re offline");
  cleanup();
  renderCard(at({ name: "typing" }), false);
  expect(screen.getByRole("status").textContent).toContain("You’re offline");
});

test("a Microsoft address gets the admin sentence and no credential field", () => {
  renderCard(at({ name: "found", found: MICROSOFT }));
  expect(screen.getByText(/the admin does/)).toBeDefined();
  expect(screen.queryByLabelText("Password")).toBeNull();
  expect(screen.queryByRole("button", { name: "Continue" })).toBeNull();
  expect(screen.getByRole("button", { name: "Change" })).toBeDefined();
});

test("a refused token marks the field with the token wording and focuses it", () => {
  renderCard(at({ name: "found", found: FASTMAIL }, "upstream_credentials"));
  const field = screen.getByLabelText("API token");
  expect(field.getAttribute("aria-invalid")).toBe("true");
  expect(screen.getByRole("alert").textContent).toContain("the token");
  expect(field).toBe(document.activeElement);
});

test("a refusal that lands while the form stays mounted moves focus to the credential field", () => {
  const reconnect = renderCard(at({ name: "reconnect", account: ROW }));
  screen.getByRole("button", { name: "Connect" }).focus();
  reconnect.update(at({ name: "reconnect", account: ROW }, "upstream_credentials"));
  expect(screen.getByLabelText("API token")).toBe(document.activeElement);
  cleanup();
  const manual = renderCard(at({ name: "manual" }));
  screen.getByRole("button", { name: "Connect" }).focus();
  manual.update(at({ name: "manual" }, "upstream_credentials"));
  expect(screen.getByLabelText("Password")).toBe(document.activeElement);
});

test("an unreachable server is named above the fields; a reconnect names the row instead", () => {
  renderCard(at({ name: "found", found: GENERIC }, "upstream_unreachable"));
  expect(screen.getByRole("alert").textContent).toContain("Couldn’t reach imap.dekker-mail.nl");
  cleanup();
  renderCard(at({ name: "reconnect", account: ROW }, "upstream_unreachable"));
  expect(screen.getByRole("alert").textContent).toContain("The server for Fastmail didn’t answer");
});

test("continue on the found step hands the flow on without connecting", () => {
  const { dispatch, connect } = renderCard(at({ name: "found", found: FASTMAIL }));
  fill("API token", "fmu1-x");
  fireEvent.submit(screen.getByLabelText("API token").closest("form") ?? document.body);
  expect(dispatch).toHaveBeenCalledWith({ type: "continue" });
  expect(connect).not.toHaveBeenCalled();
});

test("the reconnect step asks for the credential only and sends it to the row", () => {
  const { reconnect } = renderCard(at({ name: "reconnect", account: ROW }));
  expect(screen.getByRole("heading", { name: "Reconnect Fastmail" })).toBeDefined();
  expect(screen.getByLabelText("Email address")).toHaveProperty("readOnly", true);
  expect(screen.queryByRole("button", { name: "Change" })).toBeNull();
  fill("API token", "fmu1-y");
  fireEvent.click(screen.getByRole("button", { name: "Connect" }));
  expect(reconnect).toHaveBeenCalledExactlyOnceWith({ kind: "bearer", token: "fmu1-y" });
});

test("a malformed address stays on the field; a good one runs discovery", () => {
  const { flow, dispatch } = renderCard({ ...at({ name: "typing" }), address: "sanne@localhost" });
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  expect(flow.detect).not.toHaveBeenCalled();
  expect(dispatch).toHaveBeenCalledWith({ type: "failed", failure: "invalid_request" });
  cleanup();
  const good = renderCard(at({ name: "typing" }));
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  expect(good.flow.detect).toHaveBeenCalledOnce();
});
