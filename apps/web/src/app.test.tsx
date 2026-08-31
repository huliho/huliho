// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";

import { App } from "./app";

test("mounts the application shell", () => {
  render(<App />);
  expect(screen.getByRole("main")).toBeDefined();
});
