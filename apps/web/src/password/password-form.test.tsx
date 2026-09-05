// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { PasswordChangeInput } from "@huliho/core";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import { PasswordForm } from "./password-form";
import type { PasswordFormProps } from "./password-form";

const CURRENT = "the old passphrase";
const NEXT = "a brand new passphrase";
const RETRY_SECONDS = 65;

type Submit = (input: PasswordChangeInput) => void;

afterEach(cleanup);

function renderForm(overrides: Partial<PasswordFormProps> = {}): {
  onSubmit: Mock<Submit>;
  form: HTMLFormElement;
} {
  const onSubmit = vi.fn<Submit>();
  render(
    <PasswordForm
      locale="en"
      mode="change"
      pending={false}
      failure={null}
      retryRemaining={null}
      onSubmit={onSubmit}
      {...overrides}
    />,
  );
  const form = screen.getByRole("button").closest("form");
  if (form === null) {
    throw new Error("the form is not rendered");
  }
  return { onSubmit, form };
}

function fill(label: string, value: string): void {
  fireEvent.change(screen.getByLabelText(label), { target: { value } });
}

test("a matching pair inside the window goes out with the current password", () => {
  const { onSubmit, form } = renderForm();
  fill("Current password", CURRENT);
  fill("New password", NEXT);
  fill("Repeat it", NEXT);
  fireEvent.submit(form);
  expect(onSubmit).toHaveBeenCalledExactlyOnceWith({ current: CURRENT, new: NEXT });
});

test("the forced step asks for no current password and sends none", () => {
  const { onSubmit, form } = renderForm({ mode: "forced" });
  expect(screen.queryByLabelText("Current password")).toBeNull();
  expect(screen.getByRole("button").textContent).toBe("Save and continue");
  fill("New password", NEXT);
  fill("Repeat it", NEXT);
  fireEvent.submit(form);
  expect(onSubmit).toHaveBeenCalledExactlyOnceWith({ new: NEXT });
});

test("a short password stays on the page with the window named", () => {
  const { onSubmit, form } = renderForm();
  fill("Current password", CURRENT);
  fill("New password", "too short");
  fill("Repeat it", "too short");
  fireEvent.submit(form);
  expect(onSubmit).not.toHaveBeenCalled();
  expect(screen.getByRole("alert").textContent).toContain("Use 12 to 128 characters.");
  expect(screen.getByLabelText("New password")).toBe(document.activeElement);
  fill("New password", NEXT);
  expect(screen.queryByRole("alert")).toBeNull();
});

test("a repeat that differs is named on its own field", () => {
  const { onSubmit, form } = renderForm();
  fill("Current password", CURRENT);
  fill("New password", NEXT);
  fill("Repeat it", `${NEXT}!`);
  fireEvent.submit(form);
  expect(onSubmit).not.toHaveBeenCalled();
  expect(screen.getByRole("alert").textContent).toContain("don’t match");
  expect(screen.getByLabelText("Repeat it").getAttribute("aria-invalid")).toBe("true");
  expect(screen.getByLabelText("Repeat it")).toBe(document.activeElement);
});

test("a wrong current password marks that field", () => {
  renderForm({ failure: "invalid_credentials" });
  expect(screen.getByLabelText("Current password").getAttribute("aria-invalid")).toBe("true");
  expect(screen.getByRole("alert").textContent).toContain("Check your current password");
});

test("a rate limited form holds submissions and counts down", () => {
  const { onSubmit, form } = renderForm({
    failure: "rate_limited",
    retryRemaining: RETRY_SECONDS,
  });
  expect(screen.getByRole("alert").textContent).toContain("Too many attempts");
  expect(screen.getByRole("button").textContent).toMatch(/^Try again in /);
  expect(screen.getByLabelText("New password")).toHaveProperty("readOnly", true);
  fill("Current password", CURRENT);
  fill("New password", NEXT);
  fill("Repeat it", NEXT);
  fireEvent.submit(form);
  expect(onSubmit).not.toHaveBeenCalled();
});

test("a pending save keeps the button but stops repeats", () => {
  const { onSubmit, form } = renderForm({ pending: true });
  expect(screen.getByRole("button").textContent).toBe("Saving…");
  fireEvent.submit(form);
  expect(onSubmit).not.toHaveBeenCalled();
});
