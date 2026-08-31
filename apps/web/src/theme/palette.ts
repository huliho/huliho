// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Rgb } from "./contrast";
import { contrastRatio, parseHexColor } from "./contrast";

export const STABLE_ROLES = [
  "bg",
  "surface",
  "hover",
  "border",
  "border-strong",
  "border-control",
  "text",
  "text-muted",
  "accent",
  "accent-strong",
  "accent-tint",
  "danger",
  "warn",
  "success",
] as const;

export type StableRole = (typeof STABLE_ROLES)[number];

export const STABLE_TOKEN_PREFIX = "--hh-";
export const INTERNAL_TOKEN_PREFIX = "--hhx-";

const THEMES = ["light", "dark"] as const;
type ThemeName = (typeof THEMES)[number];

export interface ThemedValue {
  light: string;
  dark: string;
}

export type MergedPalette = ReadonlyMap<StableRole, ThemedValue>;

// WCAG 2 AA floors: 4.5:1 for text, 3:1 for control borders.
const TEXT_CONTRAST_MIN = 4.5;
const CONTROL_CONTRAST_MIN = 3;

const TEXT_ROLES = ["text", "text-muted", "accent", "danger", "warn", "success"] as const;
const GROUND_ROLES = ["bg", "surface", "hover", "accent-tint"] as const;
const FILL_ROLES = ["accent", "accent-strong"] as const;
const CONTROL_GROUND_ROLES = ["bg", "surface"] as const;

export function tokenName(role: StableRole): string {
  return STABLE_TOKEN_PREFIX + role;
}

function themeSide(value: ThemedValue, theme: ThemeName): string {
  return theme === "light" ? value.light : value.dark;
}

export function parseThemedValue(value: string): ThemedValue {
  const trimmed = value.trim();
  const match = /^light-dark\(\s*([^,()]+?)\s*,\s*([^,()]+?)\s*\)$/.exec(trimmed);
  const light = match?.[1];
  const dark = match?.[2];
  if (light === undefined || dark === undefined) {
    return { light: trimmed, dark: trimmed };
  }
  return { light, dark };
}

interface ContrastRule {
  foreground: StableRole;
  ground: StableRole;
  min: number;
}

function buildContrastRules(): ContrastRule[] {
  const rules: ContrastRule[] = [];
  for (const foreground of TEXT_ROLES) {
    for (const ground of GROUND_ROLES) {
      rules.push({ foreground, ground, min: TEXT_CONTRAST_MIN });
    }
  }
  for (const fill of FILL_ROLES) {
    rules.push({ foreground: "bg", ground: fill, min: TEXT_CONTRAST_MIN });
  }
  for (const ground of CONTROL_GROUND_ROLES) {
    rules.push({ foreground: "border-control", ground, min: CONTROL_CONTRAST_MIN });
  }
  return rules;
}

const CONTRAST_RULES = buildContrastRules();

function themeColors(palette: MergedPalette, theme: ThemeName): Map<StableRole, Rgb> | string {
  const colors = new Map<StableRole, Rgb>();
  for (const role of STABLE_ROLES) {
    const value = palette.get(role);
    const color = value === undefined ? null : parseHexColor(themeSide(value, theme));
    if (color === null) {
      return `${tokenName(role)} does not hold a hex color for the ${theme} theme`;
    }
    colors.set(role, color);
  }
  return colors;
}

function ruleFailure(
  rule: ContrastRule,
  colors: Map<StableRole, Rgb>,
  theme: ThemeName,
): string | null {
  const foreground = colors.get(rule.foreground);
  const ground = colors.get(rule.ground);
  if (foreground === undefined || ground === undefined) {
    return null;
  }
  const ratio = contrastRatio(foreground, ground);
  if (ratio >= rule.min) {
    return null;
  }
  const measured = ratio.toFixed(2);
  return `${rule.foreground} on ${rule.ground} in ${theme} is ${measured}:1, below ${String(rule.min)}:1`;
}

export function contrastFailures(palette: MergedPalette): string[] {
  const failures: string[] = [];
  for (const theme of THEMES) {
    const colors = themeColors(palette, theme);
    if (typeof colors === "string") {
      failures.push(colors);
      continue;
    }
    for (const rule of CONTRAST_RULES) {
      const failure = ruleFailure(rule, colors, theme);
      if (failure !== null) {
        failures.push(failure);
      }
    }
  }
  return failures;
}
