// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { KeyboardEvent, PointerEvent } from "react";
import { useRef } from "react";

import styles from "./split-divider.module.css";

// One arrow key moves the seam this far.
export const DIVIDER_STEP_PX = 16;

export interface Bounds {
  min: number;
  max: number;
}

interface SplitDividerProps {
  label: string;
  // The size of the pane before the seam, in CSS pixels.
  value: number;
  bounds: Bounds;
  // The id of the pane the seam sizes.
  controls: string;
  onChange: (value: number) => void;
  onReset: () => void;
}

interface Drag {
  pointerId: number;
  startX: number;
  from: number;
  direction: 1 | -1;
}

export function clamp(value: number, { min, max }: Bounds): number {
  return Math.round(Math.min(max, Math.max(min, value)));
}

// Which way a pointer moving right grows the pane: with the reading direction.
function directionOf(element: HTMLElement): 1 | -1 {
  return getComputedStyle(element).direction === "rtl" ? -1 : 1;
}

// Where a key takes the seam; "reset" for Enter, null for any other key.
function keyed(key: string, value: number, step: number, bounds: Bounds): number | "reset" | null {
  switch (key) {
    case "ArrowRight":
      return value + step;
    case "ArrowLeft":
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

// The draggable seam between two panes, a window splitter: arrow keys
// move it a step, Home and End take it to a bound, Enter or a double
// click puts it back.
export function SplitDivider(props: SplitDividerProps) {
  const { label, value, bounds, controls, onChange, onReset } = props;
  const drag = useRef<Drag | null>(null);
  const onPointerDown = (event: PointerEvent<HTMLDivElement>): void => {
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      from: value,
      direction: directionOf(event.currentTarget),
    };
  };
  const onPointerMove = (event: PointerEvent<HTMLDivElement>): void => {
    const held = drag.current;
    if (held !== null && held.pointerId === event.pointerId) {
      onChange(clamp(held.from + (event.clientX - held.startX) * held.direction, bounds));
    }
  };
  const release = (): void => {
    drag.current = null;
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    const step = DIVIDER_STEP_PX * directionOf(event.currentTarget);
    const next = keyed(event.key, value, step, bounds);
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
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuenow={value}
      aria-valuemin={bounds.min}
      aria-valuemax={bounds.max}
      aria-controls={controls}
      tabIndex={0}
      className={styles.divider}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={release}
      onPointerCancel={release}
      onDoubleClick={onReset}
      onKeyDown={onKeyDown}
    />
  );
}
