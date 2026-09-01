// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Link, useNavigate } from "@tanstack/react-router";
import { ArrowLeft } from "lucide-react";
import { useEffect } from "react";

import { BrandMark } from "../design-system/brand-mark";
import { LegalNotices } from "../legal/legal-notices";
import { m } from "../paraglide/messages.js";
import { getLocale } from "../paraglide/runtime.js";
import styles from "./about.module.css";

export function AboutSettings() {
  const locale = getLocale();
  const navigate = useNavigate();

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        void navigate({ to: "/" });
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [navigate]);

  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <Link to="/" className={styles.back} aria-label={m.settings_back({}, { locale })}>
          <ArrowLeft className={styles.backIcon} aria-hidden="true" />
        </Link>
        <h1 className={styles.title}>{m.settings_title({}, { locale })}</h1>
      </header>
      <div className={styles.body}>
        <nav className={styles.sidebar} aria-label={m.settings_title({}, { locale })}>
          <span className={styles.group}>{m.settings_title({}, { locale })}</span>
          <Link to="/settings/about" className={styles.entry}>
            {m.settings_about({}, { locale })}
          </Link>
        </nav>
        <main className={styles.content}>
          <section className={styles.card}>
            <BrandMark />
          </section>
          <section className={styles.card}>
            <h2 className={styles.cardTitle}>Huliho</h2>
            <LegalNotices locale={locale} align="start" />
          </section>
        </main>
      </div>
    </div>
  );
}
