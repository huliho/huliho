// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { KeyboardEvent, PointerEvent, Ref } from "react";
import { useRef } from "react";

import styles from "./split-divider.module.css";

// One arrow key moves a seam this far, unless the caller names a step.
export const DIVIDER_STEP_PX = 16;

// A vertical seam stands between two panes side by side; a horizontal
// one between a pane above and one below.
type Orientation = "vertical" | "horizontal";

export interface Bounds {
  min: number;
  max: number;
}

interface SplitDividerProps {
  // The seam's element, for a caller that measures what it takes in the flow.
  ref?: Ref<HTMLDivElement>;
  label: string;
  // The size of the pane before the seam, in CSS pixels.
  value: number;
  bounds: Bounds;
  // The id of the pane the seam sizes.
  controls: string;
  orientation?: Orientation;
  step?: number;
  onChange: (value: number) => void;
  onReset: () => void;
}

interface Drag {
  pointerId: number;
  start: number;
  from: number;
  direction: 1 | -1;
}

// The keys that grow and shrink the pane before the seam.
const GROW_KEYS = new Map<Orientation, [string, string]>([
  ["vertical", ["ArrowRight", "ArrowLeft"]],
  ["horizontal", ["ArrowDown", "ArrowUp"]],
]);

export function clamp(value: number, { min, max }: Bounds): number {
  return Math.round(Math.min(max, Math.max(min, value)));
}

// Which way a pointer moving right grows the pane: with the reading
// direction; a pointer moving down always grows the pane above it.
function directionOf(element: HTMLElement, orientation: Orientation): 1 | -1 {
  if (orientation === "horizontal") {
    return 1;
  }
  return getComputedStyle(element).direction === "rtl" ? -1 : 1;
}

function pointerAt(event: PointerEvent<HTMLDivElement>, orientation: Orientation): number {
  return orientation === "vertical" ? event.clientX : event.clientY;
}

// Where a key takes the seam; "reset" for Enter, null for any other key.
function keyed(
  key: string,
  orientation: Orientation,
  { value, step, bounds }: { value: number; step: number; bounds: Bounds },
): number | "reset" | null {
  const [grow, shrink] = GROW_KEYS.get(orientation) ?? ["", ""];
  switch (key) {
    case grow:
      return value + step;
    case shrink:
      return value - step;
    case "Home":
      return bounds.min;
    case "End":
      return bounds.max;
    case "Enter":
      return "reset";
    default:
      return null;
  }
}

interface DragHandlers {
  onPointerDown: (event: PointerEvent<HTMLDivElement>) => void;
  onPointerMove: (event: PointerEvent<HTMLDivElement>) => void;
  release: () => void;
}

// A drag of the seam: the pane's size follows the pointer from where
// it took hold, along the seam's axis and in the reading direction.
function useDrag(
  orientation: Orientation,
  value: number,
  bounds: Bounds,
  onChange: (value: number) => void,
): DragHandlers {
  const dragRef = useRef<Drag | null>(null);
  return {
    onPointerDown: (event) => {
      event.currentTarget.setPointerCapture(event.pointerId);
      dragRef.current = {
        pointerId: event.pointerId,
        start: pointerAt(event, orientation),
        from: value,
        direction: directionOf(event.currentTarget, orientation),
      };
    },
    onPointerMove: (event) => {
      const held = dragRef.current;
      if (held !== null && held.pointerId === event.pointerId) {
        const moved = (pointerAt(event, orientation) - held.start) * held.direction;
        onChange(clamp(held.from + moved, bounds));
      }
    },
    release: () => {
      dragRef.current = null;
    },
  };
}

// The draggable seam between two panes, a window splitter: arrow keys
// move it a step, Home and End take it to a bound, Enter or a double
// click puts it back.
export function SplitDivider({
  ref,
  orientation = "vertical",
  step = DIVIDER_STEP_PX,
  ...props
}: SplitDividerProps) {
  const { label, value, bounds, controls, onChange, onReset } = props;
  const drag = useDrag(orientation, value, bounds, onChange);
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    const signed = step * directionOf(event.currentTarget, orientation);
    const next = keyed(event.key, orientation, { value, step: signed, bounds });
    if (next === null) {
      return;
    }
    event.preventDefault();
    if (next === "reset") {
      onReset();
    } else {
      onChange(clamp(next, bounds));
    }
  };
  return (
    <div
      ref={ref}
      role="separator"
      aria-orientation={orientation}
      aria-label={label}
      aria-valuenow={value}
      aria-valuemin={bounds.min}
      aria-valuemax={bounds.max}
      aria-controls={controls}
      tabIndex={0}
      className={styles.divider}
      onPointerDown={drag.onPointerDown}
      onPointerMove={drag.onPointerMove}
      onPointerUp={drag.release}
      onPointerCancel={drag.release}
      onDoubleClick={onReset}
      onKeyDown={onKeyDown}
    />
  );
}
