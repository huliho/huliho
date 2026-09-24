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
// A drag that stays inside the bounds when it shrinks the pane.
const SHORT_DRAG_X = 20;
const FAR_X = 900;

beforeEach(() => {
  // jsdom has no pointer capture; the seam asks for it on every drag.
  HTMLElement.prototype.setPointerCapture = vi.fn<(pointerId: number) => void>();
});

afterEach(() => {
  cleanup();
  document.documentElement.removeAttribute("dir");
});

// The row height, as the step of a seam that moves a list by rows, from
// a height with a row of room below it.
const ROW_STEP = 52;
const START_ROWS = 480;
const GRAB_Y = 400;
const DRAG_Y = 30;

function renderSeam(orientation: "vertical" | "horizontal" = "vertical") {
  const onChange = vi.fn<(value: number) => void>();
  const onReset = vi.fn<() => void>();
  const turned = orientation === "horizontal";
  render(
    <SplitDivider
      label="Resize the list"
      value={turned ? START_ROWS : START}
      bounds={BOUNDS}
      controls={LIST_ID}
      orientation={orientation}
      step={turned ? ROW_STEP : DIVIDER_STEP_PX}
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

test("under a right-to-left document the keys and the drag run with the reading direction", () => {
  document.documentElement.dir = "rtl";
  const { seam, onChange } = renderSeam();
  fireEvent.keyDown(seam, { key: "ArrowRight" });
  expect(onChange).toHaveBeenLastCalledWith(START - DIVIDER_STEP_PX);
  fireEvent.keyDown(seam, { key: "ArrowLeft" });
  expect(onChange).toHaveBeenLastCalledWith(START + DIVIDER_STEP_PX);
  fireEvent.pointerDown(seam, { pointerId: 1, clientX: GRAB_X });
  fireEvent.pointerMove(seam, { pointerId: 1, clientX: GRAB_X + SHORT_DRAG_X });
  expect(onChange).toHaveBeenLastCalledWith(START - SHORT_DRAG_X);
});

test("a horizontal seam moves on the up and down keys by its own step and follows the pointer down", () => {
  const { seam, onChange, onReset } = renderSeam("horizontal");
  expect(seam.getAttribute("aria-orientation")).toBe("horizontal");
  expect(seam.getAttribute("aria-valuenow")).toBe(String(START_ROWS));
  fireEvent.keyDown(seam, { key: "ArrowDown" });
  expect(onChange).toHaveBeenLastCalledWith(START_ROWS + ROW_STEP);
  fireEvent.keyDown(seam, { key: "ArrowUp" });
  expect(onChange).toHaveBeenLastCalledWith(START_ROWS - ROW_STEP);
  fireEvent.keyDown(seam, { key: "ArrowRight" });
  fireEvent.keyDown(seam, { key: "ArrowLeft" });
  expect(onChange).toHaveBeenCalledTimes(2);
  fireEvent.keyDown(seam, { key: "End" });
  expect(onChange).toHaveBeenLastCalledWith(BOUNDS.max);
  fireEvent.keyDown(seam, { key: "Enter" });
  expect(onReset).toHaveBeenCalledOnce();
  fireEvent.pointerDown(seam, { pointerId: 1, clientX: GRAB_X, clientY: GRAB_Y });
  fireEvent.pointerMove(seam, { pointerId: 1, clientX: GRAB_X + DRAG_X, clientY: GRAB_Y + DRAG_Y });
  expect(onChange).toHaveBeenLastCalledWith(START_ROWS + DRAG_Y);
});
