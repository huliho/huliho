// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { DENSITIES, FONT_SIZES, LINE_HEIGHTS, THEMES } from "@huliho/core";
import type {
  DarkMail,
  Density,
  FontSize,
  LineHeight,
  Preferences,
  ReadingPane,
  Theme,
} from "@huliho/core";

// The choices the document wears as attributes and the device remembers.
export interface Appearance {
  theme: Theme;
  density: Density;
  fontSize: FontSize;
  lineHeight: LineHeight;
}

// What a user who never chose gets: the device's own scheme, the roomy
// rows, the type at its own size and the density's own leading.
export const DEFAULT_APPEARANCE: Appearance = {
  theme: "system",
  density: "comfortable",
  fontSize: "default",
  lineHeight: "default",
};

// Where a conversation opens until the user says otherwise.
export const DEFAULT_READING_PANE: ReadingPane = "right";

// A light-only message is adapted to the dark theme until the user says otherwise.
export const DEFAULT_DARK_MAIL: DarkMail = "adapt";

const THEME_KEY = "huliho-theme";
const DENSITY_KEY = "huliho-density";
const FONT_SIZE_KEY = "huliho-font-size";
const LINE_HEIGHT_KEY = "huliho-line-height";

function remembered<T extends string>(key: string, words: readonly T[]): T | undefined {
  const value = localStorage.getItem(key);
  return words.find((word) => word === value);
}

// The appearance the server's words name, a key never chosen at its default.
export function appearanceOf(preferences: Preferences): Appearance {
  return {
    theme: preferences.theme ?? DEFAULT_APPEARANCE.theme,
    density: preferences.density ?? DEFAULT_APPEARANCE.density,
    fontSize: preferences.fontSize ?? DEFAULT_APPEARANCE.fontSize,
    lineHeight: preferences.lineHeight ?? DEFAULT_APPEARANCE.lineHeight,
  };
}

// The last appearance applied on this device, so the next load starts
// there instead of flashing the default until the server answers.
export function rememberedAppearance(): Appearance {
  return {
    theme: remembered(THEME_KEY, THEMES) ?? DEFAULT_APPEARANCE.theme,
    density: remembered(DENSITY_KEY, DENSITIES) ?? DEFAULT_APPEARANCE.density,
    fontSize: remembered(FONT_SIZE_KEY, FONT_SIZES) ?? DEFAULT_APPEARANCE.fontSize,
    lineHeight: remembered(LINE_HEIGHT_KEY, LINE_HEIGHTS) ?? DEFAULT_APPEARANCE.lineHeight,
  };
}

// Sets the attributes the stylesheet reads and remembers them for the next load.
export function applyAppearance(doc: Document, appearance: Appearance): void {
  const root = doc.documentElement;
  root.dataset["theme"] = appearance.theme;
  root.dataset["density"] = appearance.density;
  root.dataset["fontSize"] = appearance.fontSize;
  root.dataset["lineHeight"] = appearance.lineHeight;
  localStorage.setItem(THEME_KEY, appearance.theme);
  localStorage.setItem(DENSITY_KEY, appearance.density);
  localStorage.setItem(FONT_SIZE_KEY, appearance.fontSize);
  localStorage.setItem(LINE_HEIGHT_KEY, appearance.lineHeight);
}
