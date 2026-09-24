// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { isPreferenceLocale } from "@huliho/core";
import { PSEUDO_LOCALE } from "@huliho/i18n";
import { useSyncExternalStore } from "react";

import {
  baseLocale,
  extractLocaleFromNavigator,
  getLocale,
  getTextDirection,
  locales,
  setLocale,
} from "../paraglide/runtime.js";
import type { Locale } from "../paraglide/runtime.js";

const listeners = new Set<() => void>();

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

// The locale a component renders in; a switch re-renders every subscriber.
export function useLocale(): Locale {
  return useSyncExternalStore(subscribe, getLocale, getLocale);
}

// The pseudo locale reads right to left, so every sweep in it shows the RTL state.
function directionOf(locale: Locale): "ltr" | "rtl" {
  return locale === PSEUDO_LOCALE ? "rtl" : getTextDirection(locale);
}

// Puts a locale on the document: its language and its writing direction.
export function applyDocumentLocale(locale: Locale): void {
  document.documentElement.lang = locale;
  document.documentElement.dir = directionOf(locale);
}

// Applies a locale to the document and to every mounted screen without a reload.
export function switchLocale(next: Locale): void {
  if (next === getLocale()) {
    return;
  }
  void setLocale(next, { reload: false });
  applyDocumentLocale(next);
  for (const listener of listeners) {
    listener();
  }
}

// The pseudo locale is a development aid, so only development builds list it.
export function listedLocales(current: Locale): Locale[] {
  const listed = import.meta.env.DEV
    ? [...locales]
    : locales.filter((locale) => locale !== PSEUDO_LOCALE);
  return listed.includes(current) ? listed : [...listed, current];
}

// The browser's own language among the ones the server stores: what a user who never chose gets.
export function preferredLocale(): Locale {
  const locale = extractLocaleFromNavigator();
  return locale !== undefined && isPreferenceLocale(locale) ? locale : baseLocale;
}
