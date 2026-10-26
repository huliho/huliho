// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useEffect, useRef } from "react";

interface Press {
  held: boolean;
  waiting: (() => void) | null;
}

// Hands `then` the element that holds the focus once a press under way
// has ended in its click, or `next` at once without a press. A blur that
// hides the shortcuts waits for it, so nothing shifts under the pointer
// between the press and its click; a tap's click comes after its release.
export function usePressEnd(): (
  next: EventTarget | null,
  then: (target: EventTarget | null) => void,
) => void {
  const press = useRef<Press>({ held: false, waiting: null });
  useEffect(() => {
    const state = press.current;
    const down = (): void => {
      state.held = true;
    };
    const end = (): void => {
      state.held = false;
      const waiting = state.waiting;
      state.waiting = null;
      waiting?.();
    };
    // The click's own handlers run first, so the wait ends after them.
    const clicked = (): void => {
      setTimeout(end, 0);
    };
    document.addEventListener("pointerdown", down, true);
    document.addEventListener("click", clicked, true);
    document.addEventListener("pointercancel", end, true);
    return () => {
      document.removeEventListener("pointerdown", down, true);
      document.removeEventListener("click", clicked, true);
      document.removeEventListener("pointercancel", end, true);
    };
  }, []);
  return (next, then) => {
    if (press.current.held) {
      press.current.waiting = () => {
        then(document.activeElement);
      };
    } else {
      then(next);
    }
  };
}
