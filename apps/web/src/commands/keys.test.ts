// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { chordOf, chordText, keysText, sameKeys } from "./keys";

const MAC = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15";
const WINDOWS = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

function onPlatform(userAgent: string): void {
  vi.spyOn(navigator, "userAgent", "get").mockReturnValue(userAgent);
}

function keydown(key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  return new KeyboardEvent("keydown", { key, ...init });
}

afterEach(() => {
  vi.restoreAllMocks();
});

test("a cap shows the modifiers as the platform draws them", () => {
  onPlatform(MAC);
  expect(chordText({ key: "k", mod: true })).toBe("⌘K");
  expect(chordText({ key: "l", mod: true, shift: true })).toBe("⌘⇧L");
  expect(keysText([{ key: "g" }, { key: "i" }])).toBe("g i");
  onPlatform(WINDOWS);
  expect(chordText({ key: "k", mod: true })).toBe("Ctrl+K");
  expect(chordText({ key: "l", mod: true, shift: true })).toBe("Ctrl+Shift+L");
});

test("a key the event names in words shows its legend", () => {
  expect(chordText({ key: "Escape" })).toBe("esc");
  expect(chordText({ key: "Enter" })).toBe("↵");
  expect(chordText({ key: "ArrowDown" })).toBe("↓");
  expect(chordText({ key: "?" })).toBe("?");
  expect(keysText([])).toBe("");
});

test("a key no command registers shows the name the event gives it", () => {
  expect(chordText({ key: "Tab" })).toBe("Tab");
  expect(chordText({ key: "ArrowLeft" })).toBe("ArrowLeft");
});

test("the command key is Cmd on a Mac and Ctrl elsewhere, never both", () => {
  onPlatform(MAC);
  expect(chordOf(keydown("k", { metaKey: true }))).toEqual({ key: "k", mod: true, shift: false });
  expect(chordOf(keydown("k", { ctrlKey: true }))).toBeNull();
  onPlatform(WINDOWS);
  expect(chordOf(keydown("K", { ctrlKey: true, shiftKey: true }))).toEqual({
    key: "k",
    mod: true,
    shift: true,
  });
  expect(chordOf(keydown("k", { metaKey: true }))).toBeNull();
  expect(chordOf(keydown("Shift", { shiftKey: true }))).toBeNull();
  expect(chordOf(keydown("j"))).toEqual({ key: "j" });
});

test("a sequence matches chord by chord", () => {
  expect(sameKeys([{ key: "g" }, { key: "i" }], [{ key: "g" }, { key: "i" }])).toBe(true);
  expect(sameKeys([{ key: "g" }, { key: "i" }], [{ key: "g" }])).toBe(false);
  expect(sameKeys([{ key: "k", mod: true }], [{ key: "K", mod: true, shift: false }])).toBe(true);
  expect(sameKeys([{ key: "k", mod: true }], [{ key: "k" }])).toBe(false);
});
