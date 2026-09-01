// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test } from "vitest";

import { App } from "./app";
import { setLocale } from "./paraglide/runtime.js";

beforeEach(async () => {
  localStorage.clear();
  await setLocale("en", { reload: false });
});

afterEach(cleanup);

test("mounts the application shell", () => {
  render(<App />);
  expect(screen.getByRole("main")).toBeDefined();
  expect(screen.getByRole("heading", { level: 1, name: "Huliho" })).toBeDefined();
});

test("switching the locale translates the page without a reload", () => {
  render(<App />);
  expect(screen.getByText("Your mail, wherever it lives.")).toBeDefined();

  fireEvent.change(screen.getByLabelText("Language"), { target: { value: "nl" } });

  expect(screen.getByText("Je mail, waar die ook staat.")).toBeDefined();
  expect(document.documentElement.lang).toBe("nl");
  expect(localStorage.getItem("PARAGLIDE_LOCALE")).toBe("nl");
});

test("dates and numbers format per locale through Intl", () => {
  render(<App />);
  expect(screen.getByText(/24,817 messages/)).toBeDefined();

  fireEvent.change(screen.getByLabelText("Language"), { target: { value: "nl" } });

  expect(screen.getByText(/24\.817 berichten/)).toBeDefined();
  expect(screen.getByText(/Vandaag is het/)).toBeDefined();
});
