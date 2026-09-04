// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Link } from "@tanstack/react-router";

import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import styles from "./settings-nav.module.css";

export interface SettingsEntry {
  to: "/settings/sessions" | "/settings/about";
  label: (locale: Locale) => string;
}

// Only pages that exist; a drawn page joins when it is built.
export const SETTINGS_ENTRIES: SettingsEntry[] = [
  { to: "/settings/sessions", label: (locale) => m.settings_sessions({}, { locale }) },
  { to: "/settings/about", label: (locale) => m.settings_about({}, { locale }) },
];

interface SettingsNavProps {
  locale: Locale;
  layout: "sidebar" | "list";
}

export function SettingsNav({ locale, layout }: SettingsNavProps) {
  return (
    <nav
      className={layout === "sidebar" ? styles.sidebar : styles.list}
      aria-label={m.settings_title({}, { locale })}
    >
      <span className={styles.group}>{m.settings_title({}, { locale })}</span>
      {SETTINGS_ENTRIES.map((entry) => (
        <Link key={entry.to} to={entry.to} className={styles.entry}>
          {entry.label(locale)}
        </Link>
      ))}
    </nav>
  );
}
