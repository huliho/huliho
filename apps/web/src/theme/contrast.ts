// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

const HEX_SHORT_LENGTH = 3;
const HEX_BASE = 16;
const CHANNEL_MAX = 255;

// Constants from the WCAG 2 relative luminance and contrast ratio definitions.
const LINEAR_THRESHOLD = 0.04045;
const LINEAR_DIVISOR = 12.92;
const GAMMA_OFFSET = 0.055;
const GAMMA_DIVISOR = 1.055;
const GAMMA_EXPONENT = 2.4;
const RED_WEIGHT = 0.2126;
const GREEN_WEIGHT = 0.7152;
const BLUE_WEIGHT = 0.0722;
const CONTRAST_OFFSET = 0.05;

export function parseHexColor(value: string): Rgb | null {
  const match = /^#([0-9A-Fa-f]{3}|[0-9A-Fa-f]{6})$/.exec(value.trim());
  const digits = match?.[1];
  if (digits === undefined) {
    return null;
  }
  const expanded =
    digits.length === HEX_SHORT_LENGTH
      ? digits
          .split("")
          .map((digit) => digit + digit)
          .join("")
      : digits;
  return {
    r: Number.parseInt(expanded.slice(0, 2), HEX_BASE),
    g: Number.parseInt(expanded.slice(2, 4), HEX_BASE),
    b: Number.parseInt(expanded.slice(4, 6), HEX_BASE),
  };
}

function linearChannel(channel: number): number {
  const scaled = channel / CHANNEL_MAX;
  return scaled <= LINEAR_THRESHOLD
    ? scaled / LINEAR_DIVISOR
    : ((scaled + GAMMA_OFFSET) / GAMMA_DIVISOR) ** GAMMA_EXPONENT;
}

function relativeLuminance(color: Rgb): number {
  return (
    RED_WEIGHT * linearChannel(color.r) +
    GREEN_WEIGHT * linearChannel(color.g) +
    BLUE_WEIGHT * linearChannel(color.b)
  );
}

export function contrastRatio(first: Rgb, second: Rgb): number {
  const a = relativeLuminance(first) + CONTRAST_OFFSET;
  const b = relativeLuminance(second) + CONTRAST_OFFSET;
  return a > b ? a / b : b / a;
}
