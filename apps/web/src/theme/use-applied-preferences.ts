// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { PSEUDO_LOCALE } from "@huliho/i18n";
import { preferencesQueryOptions } from "@huliho/state";
import { useQuery } from "@tanstack/react-query";
import { useEffect } from "react";

import { preferredLocale, switchLocale } from "../i18n/locale";
import { getLocale } from "../paraglide/runtime.js";
import { DEFAULT_APPEARANCE, applyAppearance } from "./appearance";

// The pseudo locale is the device's own development aid, never traded away.
export function useAppliedPreferences(): void {
  const { data } = useQuery(preferencesQueryOptions);
  useEffect(() => {
    if (data === undefined) {
      return;
    }
    applyAppearance(document, {
      theme: data.theme ?? DEFAULT_APPEARANCE.theme,
      density: data.density ?? DEFAULT_APPEARANCE.density,
    });
    if (getLocale() !== PSEUDO_LOCALE) {
      switchLocale(data.locale ?? preferredLocale());
    }
  }, [data]);
}
