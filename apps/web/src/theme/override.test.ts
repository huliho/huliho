// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { defaultPalette } from "./default-palette-fixture";
import type { OverrideResult } from "./override";
import { validateOverride } from "./override";
import type { MergedPalette } from "./palette";

// A violet rebrand whose pairs all clear the contrast floors.
const VIOLET_REBRAND = new Map([
  ["--hh-accent", "light-dark(#5b3ea8, #b6a3ec)"],
  ["--hh-accent-strong", "light-dark(#48318a, #cbbcf2)"],
  ["--hh-accent-tint", "light-dark(#f2eefb, #251b47)"],
]);

function message(result: OverrideResult): string {
  return result.ok ? "" : result.message;
}

function mustPass(result: OverrideResult): MergedPalette {
  if (!result.ok) {
    throw new Error(result.message);
  }
  return result.merged;
}

test("a complete accent swap with sound contrast is accepted", () => {
  const merged = mustPass(validateOverride(VIOLET_REBRAND, defaultPalette()));
  expect(merged.get("accent")).toEqual({ light: "#5b3ea8", dark: "#b6a3ec" });
  expect(merged.get("text")).toEqual(defaultPalette().get("text"));
});

test("an internal token is rejected and named", () => {
  const result = validateOverride(new Map([["--hhx-row-height", "64px"]]), defaultPalette());
  expect(result.ok).toBe(false);
  expect(message(result)).toContain("--hhx-row-height");
  expect(message(result)).toContain("internal");
});

test("an unknown token name is rejected and named", () => {
  const result = validateOverride(new Map([["--hh-brand", "#5b3ea8"]]), defaultPalette());
  expect(result.ok).toBe(false);
  expect(message(result)).toContain("--hh-brand");
  expect(message(result)).toContain("not a stable token");
});

test("a plain css property is rejected", () => {
  const result = validateOverride(new Map([["color", "#5b3ea8"]]), defaultPalette());
  expect(result.ok).toBe(false);
  expect(message(result)).toContain("color");
});

test("a value that is not hex is rejected and named", () => {
  const result = validateOverride(new Map([["--hh-danger", "crimson"]]), defaultPalette());
  expect(result.ok).toBe(false);
  expect(message(result)).toContain("--hh-danger");
  expect(message(result)).toContain("hex color");
});

test("a swapped accent without its companions is rejected", () => {
  const result = validateOverride(
    new Map([["--hh-accent", "light-dark(#5b3ea8, #b6a3ec)"]]),
    defaultPalette(),
  );
  expect(result.ok).toBe(false);
  expect(message(result)).toContain("--hh-accent-strong");
  expect(message(result)).toContain("--hh-accent-tint");
});

test("an override below the contrast bar is rejected with the failing pair", () => {
  const result = validateOverride(new Map([["--hh-text", "#cccccc"]]), defaultPalette());
  expect(result.ok).toBe(false);
  expect(message(result)).toContain("contrast bar");
  expect(message(result)).toContain("text on bg in light");
});
