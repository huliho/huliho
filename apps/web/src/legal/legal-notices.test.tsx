// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";

import { LegalNotices } from "./legal-notices";

afterEach(cleanup);

test("the notices carry every required element in English", () => {
  render(<LegalNotices locale="en" />);
  expect(screen.getByText("Huliho, by Eric Kochen")).toBeDefined();
  expect(screen.getByText("Copyright (C) 2026 Eric Kochen")).toBeDefined();
  expect(screen.getByText("This program comes with absolutely no warranty.")).toBeDefined();
  expect(screen.getByText("Licensees may convey it under the GNU AGPL, version 3.")).toBeDefined();
  expect(screen.getByRole("link", { name: "License" }).getAttribute("href")).toBe("/license");
  expect(screen.getByRole("link", { name: "Source code" }).getAttribute("href")).toBe(
    "https://github.com/huliho/huliho",
  );
});

test("the two name lines stay verbatim in Dutch", () => {
  render(<LegalNotices locale="nl" />);
  expect(screen.getByText("Huliho, by Eric Kochen")).toBeDefined();
  expect(screen.getByText("Copyright (C) 2026 Eric Kochen")).toBeDefined();
  expect(screen.getByText("Dit programma wordt geleverd zonder enige garantie.")).toBeDefined();
  expect(screen.getByRole("link", { name: "Licentie" })).toBeDefined();
  expect(screen.getByRole("link", { name: "Broncode" })).toBeDefined();
});
