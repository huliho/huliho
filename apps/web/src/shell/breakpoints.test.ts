// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { useLayout } from "./breakpoints";

const PHONE_PX = 600;
const TABLET_PX = 900;
const DESKTOP_PX = 1300;

type Listener = EventListenerOrEventListenerObject | null;

// A window of one width: every min-width query answers against it and
// hears about a move.
function fakeWindow(width: number): (next: number) => void {
  const listeners = new Set<EventListenerOrEventListenerObject>();
  const state = { width };
  vi.stubGlobal("matchMedia", (query: string) => {
    const min = Number(/\d+/u.exec(query)?.[0] ?? 0);
    return {
      get matches() {
        return state.width >= min;
      },
      media: query,
      addEventListener(_type: string, listener: Listener) {
        if (listener !== null) {
          listeners.add(listener);
        }
      },
      removeEventListener(_type: string, listener: Listener) {
        if (listener !== null) {
          listeners.delete(listener);
        }
      },
    };
  });
  return (next) => {
    state.width = next;
    const event = new Event("change");
    for (const listener of listeners) {
      if (typeof listener === "function") {
        listener(event);
      } else {
        listener.handleEvent(event);
      }
    }
  };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("the layout follows the two breakpoints as the window moves", () => {
  const resize = fakeWindow(PHONE_PX);
  const { result } = renderHook(() => useLayout());
  expect(result.current).toBe("phone");
  act(() => {
    resize(TABLET_PX);
  });
  expect(result.current).toBe("tablet");
  act(() => {
    resize(DESKTOP_PX);
  });
  expect(result.current).toBe("desktop");
  act(() => {
    resize(PHONE_PX);
  });
  expect(result.current).toBe("phone");
});
