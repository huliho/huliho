// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useEffect, useRef, useState } from "react";
import type { PointerEvent } from "react";

// How long a touch rests on a row before it picks the row: the long
// press of the platforms (half a second on iOS).
export const LONG_PRESS_MS = 500;

export interface LongPress {
  // The row a touch rests on while its press has not landed.
  pressing: number | null;
  onPointerDown: (event: PointerEvent<HTMLElement>) => void;
  // A lift, a cancel or a move away before the press lands ends it.
  onPointerEnd: () => void;
  // Whether a click is the one a landed press leaves behind; true once.
  takeClick: () => boolean;
  // Whether the last press was a touch, so a tap is told from a click.
  touched: () => boolean;
}

// A touch that rests on a row lands after LONG_PRESS_MS. A lift before
// that lands nothing, and so does a scroll, since it cancels the pointer;
// a second finger ends a pending press as it ends a swipe.
export function useLongPress(
  rowOf: (target: EventTarget) => number | null,
  onLand: (index: number) => void,
  enabled: boolean,
): LongPress {
  const [pressing, setPressing] = useState<number | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const landed = useRef(false);
  const touch = useRef(false);
  const stop = (): void => {
    if (timer.current !== null) {
      clearTimeout(timer.current);
      timer.current = null;
    }
  };
  const end = (): void => {
    stop();
    setPressing(null);
  };
  useEffect(
    () => () => {
      if (timer.current !== null) {
        clearTimeout(timer.current);
      }
    },
    [],
  );
  return {
    pressing,
    onPointerDown: (event) => {
      touch.current = event.pointerType === "touch";
      landed.current = false;
      end();
      const index = rowOf(event.target);
      if (!enabled || !touch.current || !event.isPrimary || index === null) {
        return;
      }
      setPressing(index);
      timer.current = setTimeout(() => {
        timer.current = null;
        landed.current = true;
        setPressing(null);
        onLand(index);
      }, LONG_PRESS_MS);
    },
    onPointerEnd: end,
    takeClick: () => {
      const taken = landed.current;
      landed.current = false;
      return taken;
    },
    touched: () => touch.current,
  };
}
