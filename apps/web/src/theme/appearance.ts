// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { DENSITIES, THEMES } from "@huliho/core";
import type { Density, Theme } from "@huliho/core";

export interface Appearance {
  theme: Theme;
  density: Density;
}

// What a user who never chose gets: the device's own scheme and the roomy rows.
export const DEFAULT_APPEARANCE: Appearance = { theme: "system", density: "comfortable" };

const THEME_KEY = "huliho-theme";
const DENSITY_KEY = "huliho-density";

function remembered<T extends string>(key: string, words: readonly T[]): T | undefined {
  const value = localStorage.getItem(key);
  return words.find((word) => word === value);
}

// The last appearance applied on this device, so the next load starts
// there instead of flashing the default until the server answers.
export function rememberedAppearance(): Appearance {
  return {
    theme: remembered(THEME_KEY, THEMES) ?? DEFAULT_APPEARANCE.theme,
    density: remembered(DENSITY_KEY, DENSITIES) ?? DEFAULT_APPEARANCE.density,
  };
}

// Sets the attributes the stylesheet reads and remembers them for the next load.
export function applyAppearance(doc: Document, appearance: Appearance): void {
  doc.documentElement.dataset["theme"] = appearance.theme;
  doc.documentElement.dataset["density"] = appearance.density;
  localStorage.setItem(THEME_KEY, appearance.theme);
  localStorage.setItem(DENSITY_KEY, appearance.density);
}
