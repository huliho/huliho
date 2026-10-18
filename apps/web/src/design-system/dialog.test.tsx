// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { Dialog } from "./dialog";

afterEach(() => {
  cleanup();
});

test("the popup sits in the tab order, so a tall one scrolls by key", () => {
  render(
    <Dialog open onOpenChange={vi.fn<(open: boolean) => void>()} title="Keyboard">
      <p>inside</p>
    </Dialog>,
  );
  expect(screen.getByRole("dialog", { name: "Keyboard" }).tabIndex).toBe(0);
});
