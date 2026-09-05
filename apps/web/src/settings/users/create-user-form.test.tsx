// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { NewUser } from "@huliho/core";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import { CreateUserForm } from "./create-user-form";
import type { CreateUserFormProps } from "./create-user-form";

type Submit = (user: NewUser) => void;

interface Rendered {
  onSubmit: Mock<Submit>;
  onEdit: Mock<() => void>;
  onCancel: Mock<() => void>;
  form: HTMLFormElement;
}

afterEach(cleanup);

function renderForm(overrides: Partial<CreateUserFormProps> = {}): Rendered {
  const onSubmit = vi.fn<Submit>();
  const onEdit = vi.fn<() => void>();
  const onCancel = vi.fn<() => void>();
  render(
    <CreateUserForm
      locale="en"
      roles={["member", "admin"]}
      pending={false}
      loginError={undefined}
      onSubmit={onSubmit}
      onEdit={onEdit}
      onCancel={onCancel}
      {...overrides}
    />,
  );
  const form = screen.getByLabelText("Name").closest("form");
  if (form === null) {
    throw new Error("the form is not rendered");
  }
  return { onSubmit, onEdit, onCancel, form };
}

function fill(label: string, value: string): void {
  fireEvent.change(screen.getByLabelText(label), { target: { value } });
}

test("a filled form goes out trimmed with the chosen role", () => {
  const { onSubmit, form } = renderForm();
  fill("Name", "  Jonas  ");
  fill("Sign-in name", "jonas@example.com");
  fill("Role", "admin");
  fireEvent.submit(form);
  expect(onSubmit).toHaveBeenCalledExactlyOnceWith({
    name: "Jonas",
    login: "jonas@example.com",
    role: "admin",
  });
});

test("only the offered roles are listed and the lowest is preselected", () => {
  renderForm({ roles: ["member", "admin", "owner"] });
  expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual([
    "Member",
    "Admin",
    "Owner",
  ]);
  expect(screen.getByLabelText("Role")).toHaveProperty("value", "member");
});

test("a blank name stays on the page and its field is named", () => {
  const { onSubmit, form } = renderForm();
  fill("Name", "   ");
  fill("Sign-in name", "jonas@example.com");
  fireEvent.submit(form);
  expect(onSubmit).not.toHaveBeenCalled();
  expect(screen.getByRole("alert").textContent).toContain("Enter a name");
  expect(screen.getByLabelText("Name")).toBe(document.activeElement);
  fill("Name", "Jonas");
  expect(screen.queryByRole("alert")).toBeNull();
});

test("a sign-in name with a space stays on the page and its field is named", () => {
  const { onSubmit, form } = renderForm();
  fill("Name", "Jonas");
  fill("Sign-in name", "jonas example");
  fireEvent.submit(form);
  expect(onSubmit).not.toHaveBeenCalled();
  expect(screen.getByRole("alert").textContent).toContain("without spaces");
  expect(screen.getByLabelText("Sign-in name")).toBe(document.activeElement);
  fill("Sign-in name", "jonas@example.com");
  expect(screen.queryByRole("alert")).toBeNull();
});

test("the server's word on the sign-in name shows on its field until it changes", () => {
  const { onEdit } = renderForm({ loginError: "That sign-in name is already in use." });
  expect(screen.getByLabelText("Sign-in name").getAttribute("aria-invalid")).toBe("true");
  expect(screen.getByRole("alert").textContent).toContain("already in use");
  fill("Sign-in name", "other@example.com");
  expect(onEdit).toHaveBeenCalledOnce();
});

test("cancel closes without submitting", () => {
  const { onSubmit, onCancel } = renderForm();
  fill("Name", "Jonas");
  fill("Sign-in name", "jonas@example.com");
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(onCancel).toHaveBeenCalledOnce();
  expect(onSubmit).not.toHaveBeenCalled();
});

test("a pending create shows so and takes no second submit", () => {
  const { onSubmit, form } = renderForm({ pending: true });
  expect(screen.getByRole("button", { name: "Creating…" })).toBeDefined();
  fill("Name", "Jonas");
  fill("Sign-in name", "jonas@example.com");
  fireEvent.submit(form);
  expect(onSubmit).not.toHaveBeenCalled();
});
