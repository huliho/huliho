// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { act, cleanup, render, screen } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { readFrameStyle, useFrameStyle } from "./use-frame-style";

// The colors the stub paints per token, light and dark.
const PAINTED = new Map<string, [string, string]>([
  ["--hh-bg", ["rgb(242, 245, 246)", "rgb(13, 19, 21)"]],
  ["--hh-surface", ["rgb(255, 255, 255)", "rgb(22, 29, 32)"]],
  ["--hh-border", ["rgb(223, 230, 232)", "rgb(38, 48, 52)"]],
  ["--hh-text", ["rgb(35, 43, 47)", "rgb(228, 234, 236)"]],
  ["--hh-text-muted", ["rgb(103, 109, 111)", "rgb(150, 156, 158)"]],
]);

const painting = { dark: false, fontSize: "13px" };
const media = { listeners: new Set<() => void>() };

// The test environment lays nothing out; the stub answers what a layout
// would, a probe's color by the token it wears.
function stubComputedStyle(): void {
  vi.stubGlobal("getComputedStyle", (element: Element) => {
    const worn = /var\((--[\w-]+)\)/.exec(element.getAttribute("style") ?? "")?.[1] ?? "";
    const pair = PAINTED.get(worn);
    return {
      fontFamily: "Hanken Grotesk, sans-serif",
      fontSize: painting.fontSize,
      lineHeight: "18.85px",
      color: pair === undefined ? "" : pair[painting.dark ? 1 : 0],
    };
  });
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: false,
    media: query,
    addEventListener(_type: string, listener: () => void) {
      media.listeners.add(listener);
    },
    removeEventListener(_type: string, listener: () => void) {
      media.listeners.delete(listener);
    },
  }));
}

function Harness() {
  const ref = useRef<HTMLDivElement>(null);
  const style = useFrameStyle(ref);
  return (
    <div ref={ref}>
      <output>{style === null ? "none" : `${style.fontSize} ${style.bg} ${style.text}`}</output>
    </div>
  );
}

async function documentSet(attribute: string, value: string | null): Promise<void> {
  await act(async () => {
    if (value === null) {
      document.documentElement.removeAttribute(attribute);
    } else {
      document.documentElement.setAttribute(attribute, value);
    }
    // The observer reports in a microtask of its own.
    await Promise.resolve();
  });
}

beforeEach(() => {
  painting.dark = false;
  painting.fontSize = "13px";
  media.listeners.clear();
  stubComputedStyle();
});

afterEach(async () => {
  cleanup();
  await documentSet("data-theme", null);
  await documentSet("data-density", null);
  vi.unstubAllGlobals();
});

test("the style is read from the card: its type and the five tokens as painted, the probe gone", () => {
  const host = document.createElement("div");
  document.body.append(host);
  expect(readFrameStyle(host)).toEqual({
    fontFamily: "Hanken Grotesk, sans-serif",
    fontSize: "13px",
    lineHeight: "18.85px",
    bg: "rgb(242, 245, 246)",
    surface: "rgb(255, 255, 255)",
    border: "rgb(223, 230, 232)",
    text: "rgb(35, 43, 47)",
    muted: "rgb(103, 109, 111)",
  });
  expect(host.childNodes).toHaveLength(0);
  host.remove();
});

test("the hook reads once the card is laid out and again when the theme, the system's scheme or the density changes", async () => {
  render(<Harness />);
  expect(screen.getByRole("status").textContent).toBe("13px rgb(242, 245, 246) rgb(35, 43, 47)");
  painting.dark = true;
  await documentSet("data-theme", "dark");
  expect(screen.getByRole("status").textContent).toBe("13px rgb(13, 19, 21) rgb(228, 234, 236)");
  painting.fontSize = "14px";
  await documentSet("data-density", "touch");
  expect(screen.getByRole("status").textContent).toBe("14px rgb(13, 19, 21) rgb(228, 234, 236)");
  painting.dark = false;
  act(() => {
    for (const listener of media.listeners) {
      listener();
    }
  });
  expect(screen.getByRole("status").textContent).toBe("14px rgb(242, 245, 246) rgb(35, 43, 47)");
  cleanup();
  expect(media.listeners.size).toBe(0);
});
