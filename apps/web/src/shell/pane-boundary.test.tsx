// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { PaneBoundary } from "./pane-boundary";

// Fails while told to, as a pane whose data is mended between two
// renders would.
function Flaky({ failing }: { failing: boolean }) {
  if (failing) {
    throw new Error("a render failed");
  }
  return <p>rendered</p>;
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

test("a render failure shows the error sentence and Try again renders the pane afresh", () => {
  const logged = vi.spyOn(console, "error").mockImplementation(() => undefined);
  const { rerender } = render(
    <PaneBoundary>
      <Flaky failing />
    </PaneBoundary>,
  );
  const alert = screen.getByRole("alert");
  expect(alert.textContent).toContain("Couldn’t show this part of the screen.");
  expect(logged.mock.calls).toContainEqual(["pane: a render failed", "Error"]);
  rerender(
    <PaneBoundary>
      <Flaky failing={false} />
    </PaneBoundary>,
  );
  expect(screen.getByRole("alert")).toBeDefined();
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(screen.getByText("rendered")).toBeDefined();
  expect(screen.queryByRole("alert")).toBeNull();
});
