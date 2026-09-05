// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ReactNode } from "react";

import styles from "./settings-section.module.css";

interface SettingsSectionProps {
  title?: string | undefined;
  // Sits at the end of the title row: the one thing to do with the section.
  action?: ReactNode;
  children: ReactNode;
}

export function SettingsSection({ title, action, children }: SettingsSectionProps) {
  return (
    <section className={styles.card}>
      {title !== undefined && (
        <div className={styles.cardHeader}>
          <h2 className={styles.cardTitle}>{title}</h2>
          {action}
        </div>
      )}
      {children}
    </section>
  );
}
