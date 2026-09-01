// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { SignInForm } from "./sign-in-form";

const RETRY_SECONDS = 65;

type SubmitHandler = (input: { login: string; password: string }) => void;

afterEach(cleanup);

function renderedForm(): HTMLFormElement {
  const form = screen.getByRole("button").closest("form");
  if (form === null) {
    throw new Error("the form is not rendered");
  }
  return form;
}

function fillAndSubmit(login: string, password: string): void {
  fireEvent.change(screen.getByLabelText("Name"), { target: { value: login } });
  fireEvent.change(screen.getByLabelText("Password"), { target: { value: password } });
  fireEvent.submit(renderedForm());
}

test("a filled form submits its values once", () => {
  const onSubmit = vi.fn<SubmitHandler>();
  render(
    <SignInForm
      locale="en"
      pending={false}
      failure={null}
      retryRemaining={null}
      onSubmit={onSubmit}
    />,
  );
  fillAndSubmit("mira@example.com", "example passphrase");
  expect(onSubmit).toHaveBeenCalledWith({
    login: "mira@example.com",
    password: "example passphrase",
  });
});

test("wrong credentials mark the password field and explain", () => {
  render(
    <SignInForm
      locale="en"
      pending={false}
      failure="invalid_credentials"
      retryRemaining={null}
      onSubmit={vi.fn<SubmitHandler>()}
    />,
  );
  expect(screen.getByLabelText("Password").getAttribute("aria-invalid")).toBe("true");
  expect(screen.getByRole("alert").textContent).toContain("Check the name and password");
});

test("a rate limited form holds submissions and counts down", () => {
  const onSubmit = vi.fn<SubmitHandler>();
  render(
    <SignInForm
      locale="en"
      pending={false}
      failure="rate_limited"
      retryRemaining={RETRY_SECONDS}
      onSubmit={onSubmit}
    />,
  );
  expect(screen.getByRole("alert").textContent).toContain("Too many attempts");
  const formatted = new Intl.DurationFormat("en", {
    style: "digital",
    hoursDisplay: "auto",
  }).format({ minutes: 1, seconds: 5 });
  expect(screen.getByRole("button").textContent).toBe(`Try again in ${formatted}`);
  fillAndSubmit("mira@example.com", "example passphrase");
  expect(onSubmit).not.toHaveBeenCalled();
});

test("a pending submission keeps the button but stops repeats", () => {
  const onSubmit = vi.fn<SubmitHandler>();
  render(
    <SignInForm locale="en" pending failure={null} retryRemaining={null} onSubmit={onSubmit} />,
  );
  expect(screen.getByRole("button").textContent).toBe("Signing in…");
  fireEvent.submit(renderedForm());
  expect(onSubmit).not.toHaveBeenCalled();
});
