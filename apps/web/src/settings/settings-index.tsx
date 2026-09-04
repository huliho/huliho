// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Navigate } from "@tanstack/react-router";

import { getLocale } from "../paraglide/runtime.js";
import { useSidebarShown } from "./layout";
import { SettingsNav } from "./settings-nav";

// A wide screen already lists the pages in the sidebar, so it opens the first one.
export function SettingsIndex() {
  const locale = getLocale();
  const sidebarShown = useSidebarShown();
  if (sidebarShown) {
    return <Navigate to="/settings/sessions" replace />;
  }
  return <SettingsNav locale={locale} layout="list" />;
}
