// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { mayManageUsers } from "@huliho/core";
import { sessionQueryOptions } from "@huliho/state";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";

import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import styles from "./settings-nav.module.css";

export interface SettingsEntry {
  to: "/settings/sessions" | "/settings/about" | "/settings/users";
  label: (locale: Locale) => string;
  // Admin entries show for admins and owners only.
  admin?: boolean;
}

// Only pages that exist; a drawn page joins when it is built.
export const SETTINGS_ENTRIES: SettingsEntry[] = [
  { to: "/settings/sessions", label: (locale) => m.settings_sessions({}, { locale }) },
  { to: "/settings/about", label: (locale) => m.settings_about({}, { locale }) },
  { to: "/settings/users", label: (locale) => m.settings_users({}, { locale }), admin: true },
];

interface SettingsNavProps {
  locale: Locale;
  layout: "sidebar" | "list";
}

interface NavGroupProps {
  locale: Locale;
  label: string;
  entries: SettingsEntry[];
}

function NavGroup({ locale, label, entries }: NavGroupProps) {
  return (
    <>
      <span className={styles.group}>{label}</span>
      {entries.map((entry) => (
        <Link key={entry.to} to={entry.to} className={styles.entry}>
          {entry.label(locale)}
        </Link>
      ))}
    </>
  );
}

export function SettingsNav({ locale, layout }: SettingsNavProps) {
  const role = useQuery(sessionQueryOptions).data?.user.role;
  const admin = role !== undefined && mayManageUsers(role);
  return (
    <nav
      className={layout === "sidebar" ? styles.sidebar : styles.list}
      aria-label={m.settings_title({}, { locale })}
    >
      <NavGroup
        locale={locale}
        label={m.settings_title({}, { locale })}
        entries={SETTINGS_ENTRIES.filter((entry) => entry.admin !== true)}
      />
      {admin && (
        <NavGroup
          locale={locale}
          label={m.settings_admin({}, { locale })}
          entries={SETTINGS_ENTRIES.filter((entry) => entry.admin === true)}
        />
      )}
    </nav>
  );
}
