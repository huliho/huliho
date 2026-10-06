// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { useTheme } from "./use-theme";

// A media query list the test flips, with the listeners it was given.
const media = { dark: false, listeners: new Set<() => void>() };

function stubMedia(): void {
  vi.stubGlobal("matchMedia", (query: string) => ({
    get matches() {
      return query.includes("dark") && media.dark;
    },
    media: query,
    addEventListener(_type: string, listener: () => void) {
      media.listeners.add(listener);
    },
    removeEventListener(_type: string, listener: () => void) {
      media.listeners.delete(listener);
    },
  }));
}

function Shown() {
  return <output>{useTheme()}</output>;
}

function systemPrefers(dark: boolean): void {
  media.dark = dark;
  act(() => {
    for (const listener of media.listeners) {
      listener();
    }
  });
}

async function chosen(theme: string | null): Promise<void> {
  await act(async () => {
    if (theme === null) {
      delete document.documentElement.dataset["theme"];
    } else {
      document.documentElement.dataset["theme"] = theme;
    }
    // The observer reports in a microtask of its own.
    await Promise.resolve();
  });
}

beforeEach(() => {
  media.dark = false;
  media.listeners.clear();
  stubMedia();
});

afterEach(async () => {
  cleanup();
  await chosen(null);
  vi.unstubAllGlobals();
});

test("the theme is the one the document names, else the system's; it follows both", async () => {
  render(<Shown />);
  expect(screen.getByRole("status").textContent).toBe("light");
  systemPrefers(true);
  expect(screen.getByRole("status").textContent).toBe("dark");
  await chosen("light");
  expect(screen.getByRole("status").textContent).toBe("light");
  await chosen("dark");
  expect(screen.getByRole("status").textContent).toBe("dark");
  systemPrefers(false);
  expect(screen.getByRole("status").textContent).toBe("dark");
  await chosen("system");
  expect(screen.getByRole("status").textContent).toBe("light");
});

test("a component that leaves lets go of the media query", () => {
  render(<Shown />);
  expect(media.listeners.size).toBe(1);
  cleanup();
  expect(media.listeners.size).toBe(0);
});
