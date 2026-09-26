// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { readFileSync } from "node:fs";

import { afterEach, expect, test, vi } from "vitest";

import { LEGENDS, chordOf, chordText, keysText, sameKeys } from "./keys";

const MAC = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15";
const WINDOWS = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";
// Vitest runs with apps/web as its root, so this relative path reaches the fonts file.
const FONTS_CSS = readFileSync("src/styles/fonts.css", "utf8");
// The code points the self-hosted mono faces declare, each as [first, last].
const MONO_RANGES = FONTS_CSS.split("@font-face")
  .filter((face) => face.includes('"IBM Plex Mono"'))
  .flatMap((face) => face.match(/U\+[0-9A-F-]+/gu) ?? [])
  .map((range): [number, number] => {
    const [first = "", last = first] = range.replace("U+", "").split("-");
    return [Number.parseInt(first, 16), Number.parseInt(last, 16)];
  });

function outsideMonoFonts(text: string): string[] {
  return Array.from(text).filter((char) => {
    const point = char.codePointAt(0) ?? Number.NaN;
    return !MONO_RANGES.some(([first, last]) => point >= first && point <= last);
  });
}

function onPlatform(userAgent: string): void {
  vi.spyOn(navigator, "userAgent", "get").mockReturnValue(userAgent);
}

function keydown(key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  return new KeyboardEvent("keydown", { key, ...init });
}

afterEach(() => {
  vi.restoreAllMocks();
});

test("a cap names the command key as the platform does", () => {
  onPlatform(MAC);
  expect(chordText({ key: "k", mod: true })).toBe("Cmd+K");
  expect(chordText({ key: "l", mod: true, shift: true })).toBe("Cmd+Shift+L");
  expect(keysText([{ key: "g" }, { key: "i" }])).toBe("g i");
  onPlatform(WINDOWS);
  expect(chordText({ key: "k", mod: true })).toBe("Ctrl+K");
  expect(chordText({ key: "l", mod: true, shift: true })).toBe("Ctrl+Shift+L");
});

test("a key the event names in words shows its legend", () => {
  expect(chordText({ key: "Escape" })).toBe("esc");
  expect(chordText({ key: "Enter" })).toBe("enter");
  expect(chordText({ key: "ArrowDown" })).toBe("↓");
  expect(chordText({ key: "?" })).toBe("?");
  expect(keysText([])).toBe("");
});

test("every legend and modifier is drawn from the self-hosted mono fonts", () => {
  const drawn = [...LEGENDS.values()];
  for (const platform of [MAC, WINDOWS]) {
    onPlatform(platform);
    drawn.push(chordText({ key: "l", mod: true, shift: true }));
  }
  expect(drawn.filter((text) => outsideMonoFonts(text).length > 0)).toEqual([]);
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
