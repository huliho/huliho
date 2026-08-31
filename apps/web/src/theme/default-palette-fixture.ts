// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { readFileSync } from "node:fs";

import type { MergedPalette, StableRole, ThemedValue } from "./palette";
import { STABLE_ROLES, tokenName } from "./palette";

const DECLARATION_PATTERN = /^\s*(--hh[a-z-]+):\s*light-dark\(([^,]+),([^)]+)\);$/gm;

export function defaultPalette(): MergedPalette {
  // Vitest runs with apps/web as its root, so this relative path reaches the tokens file.
  const tokensSource = readFileSync("src/styles/tokens.css", "utf8");
  const values = new Map<string, ThemedValue>();
  for (const match of tokensSource.matchAll(DECLARATION_PATTERN)) {
    const [, name, light, dark] = match;
    if (name !== undefined && light !== undefined && dark !== undefined) {
      values.set(name, { light: light.trim(), dark: dark.trim() });
    }
  }
  const palette = new Map<StableRole, ThemedValue>();
  for (const role of STABLE_ROLES) {
    const value = values.get(tokenName(role));
    if (value === undefined) {
      throw new Error(`${tokenName(role)} is missing from tokens.css`);
    }
    palette.set(role, value);
  }
  return palette;
}
