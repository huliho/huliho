// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { parseHexColor } from "./contrast";
import type { MergedPalette, StableRole, ThemedValue } from "./palette";
import {
  contrastFailures,
  INTERNAL_TOKEN_PREFIX,
  parseThemedValue,
  STABLE_ROLES,
  STABLE_TOKEN_PREFIX,
  tokenName,
} from "./palette";

export type OverrideResult = { ok: true; merged: MergedPalette } | { ok: false; message: string };

const ACCENT_COMPANIONS: readonly StableRole[] = ["accent-strong", "accent-tint"];

function roleForToken(name: string): StableRole | null {
  if (!name.startsWith(STABLE_TOKEN_PREFIX)) {
    return null;
  }
  const role = name.slice(STABLE_TOKEN_PREFIX.length);
  const known = STABLE_ROLES.find((candidate) => candidate === role);
  return known ?? null;
}

function nameFailure(name: string): string | null {
  if (name.startsWith(INTERNAL_TOKEN_PREFIX)) {
    return `${name} is an internal token; an override may only set the stable ${STABLE_TOKEN_PREFIX} tokens`;
  }
  if (roleForToken(name) === null) {
    return `${name} is not a stable token; an override may only set the stable ${STABLE_TOKEN_PREFIX} tokens`;
  }
  return null;
}

function valueFailure(name: string, value: ThemedValue): string | null {
  if (parseHexColor(value.light) === null || parseHexColor(value.dark) === null) {
    return `${name} must hold a hex color or a light-dark() pair of hex colors`;
  }
  return null;
}

function collectRoles(
  declarations: ReadonlyMap<string, string>,
): Map<StableRole, ThemedValue> | string {
  const overrides = new Map<StableRole, ThemedValue>();
  for (const [name, raw] of declarations) {
    const failure = nameFailure(name);
    if (failure !== null) {
      return failure;
    }
    const role = roleForToken(name);
    if (role === null) {
      continue;
    }
    const value = parseThemedValue(raw);
    const badValue = valueFailure(name, value);
    if (badValue !== null) {
      return badValue;
    }
    overrides.set(role, value);
  }
  return overrides;
}

function accentFailure(overrides: Map<StableRole, ThemedValue>): string | null {
  if (!overrides.has("accent")) {
    return null;
  }
  const missing = ACCENT_COMPANIONS.filter((role) => !overrides.has(role));
  if (missing.length === 0) {
    return null;
  }
  const names = missing.map((role) => tokenName(role)).join(" and ");
  return `a swapped accent must bring its own ${names}`;
}

export function validateOverride(
  declarations: ReadonlyMap<string, string>,
  base: MergedPalette,
): OverrideResult {
  const overrides = collectRoles(declarations);
  if (typeof overrides === "string") {
    return { ok: false, message: overrides };
  }
  const missingAccent = accentFailure(overrides);
  if (missingAccent !== null) {
    return { ok: false, message: missingAccent };
  }
  const merged = new Map(base);
  for (const [role, value] of overrides) {
    merged.set(role, value);
  }
  const failures = contrastFailures(merged);
  if (failures.length > 0) {
    return { ok: false, message: `the override fails the contrast bar: ${failures.join("; ")}` };
  }
  return { ok: true, merged };
}
