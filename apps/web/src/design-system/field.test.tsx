// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";

import { Field } from "./field";

afterEach(cleanup);

test("an error marks the input invalid, describes it and is announced", () => {
  render(<Field label="Password" type="password" error="Wrong password." />);
  const input = screen.getByLabelText("Password");
  const alert = screen.getByRole("alert");
  expect(input.getAttribute("aria-invalid")).toBe("true");
  expect(alert.textContent).toContain("Wrong password.");
  expect(input.getAttribute("aria-describedby")?.split(" ")).toContain(alert.id);
});

test("without an error the input is plain", () => {
  render(<Field label="Name" type="text" autoComplete="username" />);
  const input = screen.getByLabelText("Name");
  expect(input.getAttribute("aria-invalid")).toBeNull();
  expect(input.getAttribute("autocomplete")).toBe("username");
  expect(screen.queryByRole("alert")).toBeNull();
});
