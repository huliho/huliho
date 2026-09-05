// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ReactNode } from "react";

import { cx } from "./cx";
import styles from "./badge.module.css";

interface BadgeProps {
  // The words carry the meaning; the tone only sets a state apart.
  tone?: "accent" | "neutral";
  children: ReactNode;
}

export function Badge({ tone = "neutral", children }: BadgeProps) {
  return (
    <span className={cx(styles.badge, tone === "accent" ? styles.accent : styles.neutral)}>
      {children}
    </span>
  );
}
