// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Link, Outlet, useMatchRoute, useNavigate } from "@tanstack/react-router";
import { ArrowLeft } from "lucide-react";

import { useCommand } from "../commands/use-command";
import { m } from "../paraglide/messages.js";
import { getLocale } from "../paraglide/runtime.js";
import { useSidebarShown } from "./layout";
import { SETTINGS_ENTRIES, SettingsNav } from "./settings-nav";
import styles from "./settings-page.module.css";

export function SettingsPage() {
  const locale = getLocale();
  const navigate = useNavigate();
  const matchRoute = useMatchRoute();
  const sidebarShown = useSidebarShown();
  const atIndex = matchRoute({ to: "/settings" }) !== false;
  const page = SETTINGS_ENTRIES.find((entry) => matchRoute({ to: entry.to }) !== false);
  useCommand({
    id: "settings.close",
    key: "Escape",
    run: () => {
      void navigate({ to: "/" });
    },
  });
  // A phone shows one page at a time, so its header names the page and
  // its back arrow returns to the list.
  const title =
    sidebarShown || page === undefined ? m.settings_title({}, { locale }) : page.label(locale);
  const backTo = sidebarShown || atIndex ? "/" : "/settings";
  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <Link to={backTo} className={styles.back} aria-label={m.settings_back({}, { locale })}>
          <ArrowLeft className={styles.backIcon} aria-hidden="true" />
        </Link>
        <h1 className={styles.title}>{title}</h1>
      </header>
      <div className={styles.body}>
        <SettingsNav locale={locale} layout="sidebar" />
        <main className={styles.content}>
          <Outlet />
        </main>
      </div>
    </div>
  );
}
