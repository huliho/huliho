// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { defaultPalette } from "./default-palette-fixture";
import { contrastFailures, parseThemedValue } from "./palette";

test("a light-dark() value splits into its two sides", () => {
  expect(parseThemedValue("light-dark(#1e7688, #56b8c9)")).toEqual({
    light: "#1e7688",
    dark: "#56b8c9",
  });
  expect(parseThemedValue("  light-dark( #fff , #000 )  ")).toEqual({
    light: "#fff",
    dark: "#000",
  });
});

test("a single value applies to both themes", () => {
  expect(parseThemedValue("#1e7688")).toEqual({ light: "#1e7688", dark: "#1e7688" });
});

test("every contrast pair of the default palette meets its floor in both themes", () => {
  expect(contrastFailures(defaultPalette())).toEqual([]);
});

test("a palette with washed-out text names the failing pairs", () => {
  const palette = new Map(defaultPalette());
  palette.set("text", { light: "#cccccc", dark: "#333333" });
  const failures = contrastFailures(palette);
  expect(failures.some((failure) => failure.includes("text on bg in light"))).toBe(true);
  expect(failures.some((failure) => failure.includes("text on surface in dark"))).toBe(true);
});
