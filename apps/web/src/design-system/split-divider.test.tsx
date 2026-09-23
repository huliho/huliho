// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { DIVIDER_STEP_PX, SplitDivider } from "./split-divider";

const BOUNDS = { min: 320, max: 800 };
const LIST_ID = "list";
const START = 360;
const GRAB_X = 100;
const DRAG_X = 50;
const FAR_X = 900;

beforeEach(() => {
  // jsdom has no pointer capture; the seam asks for it on every drag.
  HTMLElement.prototype.setPointerCapture = vi.fn<(pointerId: number) => void>();
});

afterEach(cleanup);

function renderSeam() {
  const onChange = vi.fn<(value: number) => void>();
  const onReset = vi.fn<() => void>();
  render(
    <SplitDivider
      label="Resize the list"
      value={START}
      bounds={BOUNDS}
      controls={LIST_ID}
      onChange={onChange}
      onReset={onReset}
    />,
  );
  return { seam: screen.getByRole("separator", { name: "Resize the list" }), onChange, onReset };
}

test("the arrow keys move the seam a step, Home and End take it to the bounds and Enter resets", () => {
  const { seam, onChange, onReset } = renderSeam();
  expect(seam.getAttribute("aria-valuenow")).toBe(String(START));
  expect(seam.getAttribute("aria-valuemin")).toBe(String(BOUNDS.min));
  expect(seam.getAttribute("aria-valuemax")).toBe(String(BOUNDS.max));
  expect(seam.getAttribute("aria-controls")).toBe(LIST_ID);
  fireEvent.keyDown(seam, { key: "ArrowRight" });
  expect(onChange).toHaveBeenLastCalledWith(START + DIVIDER_STEP_PX);
  fireEvent.keyDown(seam, { key: "ArrowLeft" });
  expect(onChange).toHaveBeenLastCalledWith(START - DIVIDER_STEP_PX);
  fireEvent.keyDown(seam, { key: "Home" });
  expect(onChange).toHaveBeenLastCalledWith(BOUNDS.min);
  fireEvent.keyDown(seam, { key: "End" });
  expect(onChange).toHaveBeenLastCalledWith(BOUNDS.max);
  fireEvent.keyDown(seam, { key: "Enter" });
  expect(onReset).toHaveBeenCalledOnce();
  fireEvent.keyDown(seam, { key: "Escape" });
  expect(onChange).toHaveBeenCalledTimes(4);
});

test("a drag follows its pointer inside the bounds and ends on release", () => {
  const { seam, onChange, onReset } = renderSeam();
  fireEvent.pointerDown(seam, { pointerId: 1, clientX: GRAB_X });
  fireEvent.pointerMove(seam, { pointerId: 1, clientX: GRAB_X + DRAG_X });
  expect(onChange).toHaveBeenLastCalledWith(START + DRAG_X);
  fireEvent.pointerMove(seam, { pointerId: 1, clientX: FAR_X });
  expect(onChange).toHaveBeenLastCalledWith(BOUNDS.max);
  fireEvent.pointerMove(seam, { pointerId: 2, clientX: 0 });
  fireEvent.pointerUp(seam, { pointerId: 1 });
  fireEvent.pointerMove(seam, { pointerId: 1, clientX: GRAB_X });
  expect(onChange).toHaveBeenCalledTimes(2);
  fireEvent.doubleClick(seam);
  expect(onReset).toHaveBeenCalledOnce();
});
