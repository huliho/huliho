// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ReactNode } from "react";

import styles from "./settings-section.module.css";

interface SettingsSectionProps {
  title?: string | undefined;
  // An id on the title, so a control inside can name it as its label.
  titleId?: string | undefined;
  // Sits at the end of the title row: the one thing to do with the section.
  action?: ReactNode;
  children: ReactNode;
}

export function SettingsSection({ title, titleId, action, children }: SettingsSectionProps) {
  return (
    <section className={styles.card}>
      {title !== undefined && (
        <div className={styles.cardHeader}>
          <h2 id={titleId} className={styles.cardTitle}>
            {title}
          </h2>
          {action}
        </div>
      )}
      {children}
    </section>
  );
}
