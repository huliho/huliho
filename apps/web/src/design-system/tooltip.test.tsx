// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { Tooltip } from "./tooltip";

// Base UI waits this long under a resting pointer before a hint opens.
const HOVER_DELAY_MS = 600;

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

test("the hint names the label and the keys while the control keeps its own name", () => {
  render(
    <Tooltip label="Archive" keys={[{ key: "e" }]} defaultOpen>
      <button type="button" aria-label="Archive" />
    </Tooltip>,
  );
  expect(screen.getByRole("button", { name: "Archive" })).toBeDefined();
  const hint = screen.getByText("Archive");
  expect(hint.textContent).toBe("Archivee");
  // The key in the hint is drawn and never read out a second time.
  expect(hint.querySelector("kbd")?.getAttribute("aria-hidden")).toBe("true");
});

test("a control without keys gets a hint with its label alone", () => {
  render(
    <Tooltip label="Remove star" keys={[]} defaultOpen>
      <button type="button" aria-label="Remove star" />
    </Tooltip>,
  );
  expect(screen.getByText("Remove star").querySelector("kbd")).toBeNull();
});

test("a resting pointer opens the hint after the delay", () => {
  vi.useFakeTimers();
  render(
    <Tooltip label="Delete" keys={[{ key: "#" }]}>
      <button type="button" aria-label="Delete" />
    </Tooltip>,
  );
  const control = screen.getByRole("button", { name: "Delete" });
  fireEvent.pointerEnter(control, { pointerType: "mouse" });
  fireEvent.mouseEnter(control);
  fireEvent.mouseMove(control);
  expect(screen.queryByText("Delete")).toBeNull();
  act(() => {
    vi.advanceTimersByTime(HOVER_DELAY_MS);
  });
  expect(screen.getByText("Delete").textContent).toBe("Delete#");
});
