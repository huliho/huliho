// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cx } from "./cx";
import styles from "./brand-mark.module.css";

// Instance branding config replaces the fixed name when the wizard lands.
const INSTANCE_NAME = "Huliho";

interface BrandMarkProps {
  heading?: boolean;
  stacked?: boolean;
}

export function BrandMark({ heading = false, stacked = false }: BrandMarkProps) {
  return (
    <div className={cx(styles.mark, stacked ? styles.stacked : undefined)}>
      <span className={styles.logo} aria-hidden="true" />
      {heading ? (
        <h1 className={styles.name}>{INSTANCE_NAME}</h1>
      ) : (
        <span className={styles.name}>{INSTANCE_NAME}</span>
      )}
    </div>
  );
}
