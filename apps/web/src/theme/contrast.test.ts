// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import type { Rgb } from "./contrast";
import { contrastRatio, parseHexColor } from "./contrast";

function mustParse(value: string): Rgb {
  const color = parseHexColor(value);
  if (color === null) {
    throw new Error(`${value} did not parse as a hex color`);
  }
  return color;
}

const WHITE = { r: 255, g: 255, b: 255 };
const BLACK = { r: 0, g: 0, b: 0 };
const MAX_CONTRAST = 21;
const EQUAL_CONTRAST = 1;
// The lightest gray that still reaches 4.5:1 on white, a common WCAG reference point.
const GRAY_ON_WHITE = 4.54;
const RATIO_PRECISION = 2;

test("parses long and short hex colors", () => {
  expect(parseHexColor("#0d1315")).toEqual({ r: 13, g: 19, b: 21 });
  expect(parseHexColor("#fff")).toEqual(WHITE);
  expect(parseHexColor("  #FFF  ")).toEqual(WHITE);
});

test("rejects everything that is not a hex color", () => {
  expect(parseHexColor("teal")).toBeNull();
  expect(parseHexColor("#12")).toBeNull();
  expect(parseHexColor("#12345")).toBeNull();
  expect(parseHexColor("rgb(1, 2, 3)")).toBeNull();
  expect(parseHexColor("")).toBeNull();
});

test("computes the WCAG contrast ratio in either argument order", () => {
  expect(contrastRatio(BLACK, WHITE)).toBeCloseTo(MAX_CONTRAST, RATIO_PRECISION);
  expect(contrastRatio(WHITE, BLACK)).toBeCloseTo(MAX_CONTRAST, RATIO_PRECISION);
  expect(contrastRatio(WHITE, WHITE)).toBeCloseTo(EQUAL_CONTRAST, RATIO_PRECISION);
  expect(contrastRatio(mustParse("#767676"), WHITE)).toBeCloseTo(GRAY_ON_WHITE, RATIO_PRECISION);
});
