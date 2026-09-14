// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow, FoundServer, SignInProvider } from "@huliho/core";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import { AddAccountCard } from "./add-account-card";
import type { ConsentRefusal, FlowState, Step } from "./flow";
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
const GMAIL: FoundServer = {
  ...GENERIC,
  provider: "gmail",
  credentialKind: "appPassword",
  host: "imap.gmail.com",
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
const OAUTH_ROW: AccountRow = {
  ...ROW,
  address: "sanne@gmail.com",
  name: "Gmail",
  provider: "gmail",
  kind: "imap",
  authMethod: "oauth2",
};

interface Rendered {
  flow: AddAccountFlow;
  dispatch: Mock<AddAccountFlow["dispatch"]>;
  connect: Mock<(input: ConnectInput) => void>;
  reconnect: Mock<AddAccountFlow["reconnect"]>;
  startConsent: Mock<AddAccountFlow["startConsent"]>;
  openConsentWindow: Mock<() => void>;
  cancelConsent: Mock<() => void>;
  usePassword: Mock<() => void>;
  // Renders the same card again with another state, as a refusal would.
  update: (state: FlowState) => void;
}

afterEach(cleanup);

function renderCard(
  state: FlowState,
  online = true,
  signInProviders: SignInProvider[] = [],
): Rendered {
  const dispatch = vi.fn<AddAccountFlow["dispatch"]>();
  const connect = vi.fn<(input: ConnectInput) => void>();
  const reconnect = vi.fn<AddAccountFlow["reconnect"]>();
  const startConsent = vi.fn<AddAccountFlow["startConsent"]>();
  const openConsentWindow = vi.fn<() => void>();
  const cancelConsent = vi.fn<() => void>();
  const usePassword = vi.fn<() => void>();
  const flow: AddAccountFlow = {
    state,
    retryRemaining: null,
    dispatch,
    detect: vi.fn<() => void>(),
    connect,
    reconnect,
    startConsent,
    openConsentWindow,
    cancelConsent,
    usePassword,
  };
  const { rerender } = render(
    <AddAccountCard locale="en" flow={flow} online={online} signInProviders={signInProviders} />,
  );
  const update = (next: FlowState): void => {
    rerender(
      <AddAccountCard
        locale="en"
        flow={{ ...flow, state: next }}
        online={online}
        signInProviders={signInProviders}
      />,
    );
  };
  return {
    flow,
    dispatch,
    connect,
    reconnect,
    startConsent,
    openConsentWindow,
    cancelConsent,
    usePassword,
    update,
  };
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

test("the first screen offers the sign-in providers the session lists; without any it says none is set up", () => {
  renderCard(at({ name: "typing" }), true, ["google", "microsoft"]);
  expect(screen.getByRole("button", { name: "Continue with Google" })).toBeDefined();
  expect(screen.getByRole("button", { name: "Continue with Microsoft" })).toBeDefined();
  expect(screen.queryByText(/not set up here/)).toBeNull();
  cleanup();
  renderCard(at({ name: "typing" }));
  expect(screen.queryByRole("button", { name: /Continue with/ })).toBeNull();
  expect(screen.getByText(/not set up here/)).toBeDefined();
});

test("a sign-in button checks the address first and then starts the consent", () => {
  const bad = renderCard({ ...at({ name: "typing" }), address: "sanne@localhost" }, true, [
    "google",
  ]);
  fireEvent.click(screen.getByRole("button", { name: "Continue with Google" }));
  expect(bad.startConsent).not.toHaveBeenCalled();
  expect(bad.dispatch).toHaveBeenCalledWith({ type: "failed", failure: "invalid_request" });
  cleanup();
  const good = renderCard(at({ name: "typing" }), true, ["google"]);
  fireEvent.click(screen.getByRole("button", { name: "Continue with Google" }));
  expect(good.startConsent).toHaveBeenCalledExactlyOnceWith("google");
});

test("a found Gmail address offers Google above the app password; Microsoft gets the button in place of the admin sentence", () => {
  const gmail = renderCard(at({ name: "found", found: { ...GMAIL, oauthAvailable: true } }));
  const button = screen.getByRole("button", { name: "Continue with Google" });
  const field = screen.getByLabelText("App password");
  expect(field.compareDocumentPosition(button)).toBe(Node.DOCUMENT_POSITION_PRECEDING);
  fireEvent.click(button);
  expect(gmail.startConsent).toHaveBeenCalledExactlyOnceWith("google");
  cleanup();
  renderCard(at({ name: "found", found: { ...MICROSOFT, oauthAvailable: true } }));
  expect(screen.getByRole("button", { name: "Continue with Microsoft" })).toBe(
    document.activeElement,
  );
  expect(screen.queryByText(/the admin does/)).toBeNull();
  expect(screen.queryByLabelText("Password")).toBeNull();
  cleanup();
  renderCard(at({ name: "found", found: GMAIL }));
  expect(screen.queryByRole("button", { name: /Continue with/ })).toBeNull();
});

test("the consent step says the window is open, notes Google's testing mode and cancels; a closed window gets its button", () => {
  const open = renderCard(
    at({ name: "consent", signIn: "google", from: { name: "typing" }, id: "s1", opened: true }),
  );
  expect(screen.getByRole("status").textContent).toContain("A Google window is open");
  expect(screen.getByText(/testing mode/)).toBeDefined();
  expect(screen.getByRole("button", { name: "Cancel" })).toBe(document.activeElement);
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(open.cancelConsent).toHaveBeenCalledOnce();
  cleanup();
  const blocked = renderCard(
    at({ name: "consent", signIn: "microsoft", from: { name: "typing" }, id: null, opened: false }),
  );
  expect(screen.getByRole("status").textContent).toContain("kept the Microsoft window closed");
  expect(screen.queryByText(/testing mode/)).toBeNull();
  const button = screen.getByRole("button", { name: "Open the Microsoft window" });
  expect(button).toBe(document.activeElement);
  // Nothing to open until the start answers.
  expect(button.getAttribute("aria-disabled")).toBe("true");
  blocked.update(
    at({ name: "consent", signIn: "microsoft", from: { name: "typing" }, id: "s1", opened: false }),
  );
  expect(button.getAttribute("aria-disabled")).toBe("false");
  fireEvent.click(button);
  expect(blocked.openConsentWindow).toHaveBeenCalledOnce();
  blocked.update(
    at({ name: "consent", signIn: "microsoft", from: { name: "typing" }, id: "s1", opened: true }),
  );
  expect(screen.getByRole("button", { name: "Cancel" })).toBe(document.activeElement);
});

test("a denied consent names its cause; Google offers the password route where Microsoft and a reconnect do not", () => {
  const cases: [ConsentRefusal, RegExp][] = [
    ["accessDenied", /Google didn’t grant access.*app password/],
    ["upstreamCredentials", /isn’t sanne@fastmail.com/],
    ["smtpAuthUnavailable", /turn on SMTP AUTH/],
    ["gone", /wasn’t finished in time/],
    ["exchangeFailed", /didn’t complete/],
  ];
  for (const [cause, sentence] of cases) {
    const denied = renderCard(
      at({ name: "consentDenied", signIn: "google", from: { name: "typing" }, cause }),
    );
    expect(screen.getByRole("alert").textContent).toMatch(sentence);
    expect(screen.getByRole("button", { name: "Try again" })).toBe(document.activeElement);
    fireEvent.click(screen.getByRole("button", { name: "Use a password instead" }));
    expect(denied.usePassword).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(denied.startConsent).toHaveBeenCalledExactlyOnceWith("google");
    cleanup();
  }
  renderCard(
    at({
      name: "consentDenied",
      signIn: "microsoft",
      from: { name: "typing" },
      cause: "accessDenied",
    }),
  );
  expect(screen.getByRole("alert").textContent).toMatch(/You can try again\.$/);
  expect(screen.queryByRole("button", { name: "Use a password instead" })).toBeNull();
  cleanup();
  renderCard(
    at({
      name: "consentDenied",
      signIn: "google",
      from: { name: "reconnect", account: OAUTH_ROW },
      cause: "accessDenied",
    }),
  );
  expect(screen.getByRole("heading", { name: "Reconnect Gmail" })).toBeDefined();
  expect(screen.queryByRole("button", { name: "Use a password instead" })).toBeNull();
});

test("an OAuth row reconnects through its provider's button; without the provider it says none is set up", () => {
  const offered = renderCard(at({ name: "reconnect", account: OAUTH_ROW }), true, ["google"]);
  expect(screen.getByRole("heading", { name: "Reconnect Gmail" })).toBeDefined();
  expect(screen.queryByLabelText(/password/i)).toBeNull();
  expect(screen.getByRole("button", { name: "Continue with Google" })).toBe(document.activeElement);
  fireEvent.click(screen.getByRole("button", { name: "Continue with Google" }));
  expect(offered.startConsent).toHaveBeenCalledExactlyOnceWith("google");
  cleanup();
  renderCard(at({ name: "reconnect", account: OAUTH_ROW }));
  expect(screen.queryByRole("button", { name: /Continue with/ })).toBeNull();
  expect(screen.getByText(/not set up here/)).toBeDefined();
});

test("an instance that cannot start the consent says so above the fields", () => {
  renderCard(at({ name: "found", found: GMAIL }, "provider_not_configured"));
  expect(screen.getByRole("alert").textContent).toContain("not set up here");
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
