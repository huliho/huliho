// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { CSRF_HEADERS } from "./http";
import { z } from "./schema";

const PREFERENCES_ENDPOINT = "/api/preferences";

// The words the server takes per key; it refuses anything else.
const READING_PANES = ["right", "bottom", "off"] as const;
export const THEMES = ["system", "light", "dark"] as const;
export const DENSITIES = ["comfortable", "compact"] as const;
const PREFERENCE_LOCALES = ["en", "nl"] as const;

const preferencesSchema = z.object({
  readingPane: z.enum(READING_PANES).optional(),
  theme: z.enum(THEMES).optional(),
  density: z.enum(DENSITIES).optional(),
  locale: z.enum(PREFERENCE_LOCALES).optional(),
});

export type ReadingPane = (typeof READING_PANES)[number];
export type Theme = (typeof THEMES)[number];
export type Density = (typeof DENSITIES)[number];
export type PreferenceLocale = (typeof PREFERENCE_LOCALES)[number];
// The user's choices; a key never chosen is absent.
export type Preferences = z.infer<typeof preferencesSchema>;

// One choice: the key and the word it takes.
export type PreferenceChange =
  | { key: "readingPane"; value: ReadingPane }
  | { key: "theme"; value: Theme }
  | { key: "density"; value: Density }
  | { key: "locale"; value: PreferenceLocale };

export type PreferencesFailureCode = "unauthenticated" | "unavailable";

export class PreferencesError extends Error {
  readonly code: PreferencesFailureCode;

  constructor(code: PreferencesFailureCode) {
    super(`preferences request failed: ${code}`);
    this.name = "PreferencesError";
    this.code = code;
  }
}

export function isPreferenceLocale(locale: string): locale is PreferenceLocale {
  return PREFERENCE_LOCALES.some((word) => word === locale);
}

// The choices with one of them changed.
export function withPreference(current: Preferences, change: PreferenceChange): Preferences {
  switch (change.key) {
    case "readingPane":
      return { ...current, readingPane: change.value };
    case "theme":
      return { ...current, theme: change.value };
    case "density":
      return { ...current, density: change.value };
    default:
      return { ...current, locale: change.value };
  }
}

export async function fetchPreferences(): Promise<Preferences> {
  const response = await fetch(PREFERENCES_ENDPOINT);
  if (!response.ok) {
    throw new Error(`the preferences request failed with status ${String(response.status)}`);
  }
  return preferencesSchema.parse(await response.json());
}

// A session that ended is the one refusal the caller acts on; every
// other refusal reads as unavailable.
export async function setPreference(change: PreferenceChange): Promise<void> {
  let response: Response;
  try {
    response = await fetch(`${PREFERENCES_ENDPOINT}/${change.key}`, {
      method: "PUT",
      headers: { "content-type": "application/json", ...CSRF_HEADERS },
      body: JSON.stringify({ value: change.value }),
    });
  } catch {
    throw new PreferencesError("unavailable");
  }
  if (response.ok) {
    return;
  }
  throw new PreferencesError(response.status === 401 ? "unauthenticated" : "unavailable");
}
