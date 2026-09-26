// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Skeleton } from "../design-system/skeleton";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import styles from "./thread-pane.module.css";

// Still cards that stand for the thread while it loads.
const LOADING_CARDS = 2;

// The title, the count and the still cards: what stands where the
// thread will be while it, or the pane's own code, is on its way.
export function ThreadSkeleton({ locale }: { locale: Locale }) {
  return (
    <div role="status" aria-label={m.loading_label({}, { locale })} className={styles.thread}>
      <Skeleton className={styles.titleBar} />
      <Skeleton className={styles.countBar} />
      {Array.from({ length: LOADING_CARDS }, (_, index) => (
        <Skeleton key={index} className={styles.cardBar} />
      ))}
    </div>
  );
}
