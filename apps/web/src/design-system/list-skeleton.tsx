// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import styles from "./list-skeleton.module.css";
import rows from "./row-list.module.css";
import { Skeleton } from "./skeleton";

interface ListSkeletonProps {
  locale: Locale;
  rows: number;
}

// Still rows of two lines and a button, the shape a list has while it
// loads. Spans only: an output element takes phrasing content.
export function ListSkeleton({ locale, rows: count }: ListSkeletonProps) {
  return (
    <output className={rows.list} aria-label={m.loading_label({}, { locale })}>
      {Array.from({ length: count }, (_, index) => (
        <span key={index} className={rows.row}>
          <span className={rows.facts}>
            <Skeleton className={styles.title} />
            <Skeleton className={styles.detail} />
          </span>
          <Skeleton className={styles.button} />
        </span>
      ))}
    </output>
  );
}
