// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ReactNode } from "react";

import styles from "./settings-section.module.css";

interface SettingsSectionProps {
  title?: string | undefined;
  children: ReactNode;
}

export function SettingsSection({ title, children }: SettingsSectionProps) {
  return (
    <section className={styles.card}>
      {title !== undefined && <h2 className={styles.cardTitle}>{title}</h2>}
      {children}
    </section>
  );
}
