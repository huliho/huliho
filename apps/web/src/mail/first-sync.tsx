// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { FirstSync } from "@huliho/core";
import { useRef } from "react";

import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { useStatusText } from "./status-text";
import styles from "./first-sync.module.css";

const PERCENT = 100;

interface FirstSyncBlockProps {
  locale: Locale;
  // The sync's progress while it runs and for the fade after; null once
  // the block has gone.
  shown: FirstSync | null;
  leaving: boolean;
}

// The foot of the list while the first header sync runs: the sentence,
// the count and a hairline at synced over total. The status region is
// always in the DOM and the sentence lands in it once the block renders.
export function FirstSyncBlock({ locale, shown, leaving }: FirstSyncBlockProps) {
  const sentenceRef = useRef<HTMLParagraphElement>(null);
  useStatusText(sentenceRef, shown === null ? "" : m.list_first_sync({}, { locale }));
  const percent = shown === null ? 0 : Math.round((shown.synced / shown.total) * PERCENT);
  return (
    <div
      className={styles.block}
      data-idle={shown === null || undefined}
      data-leaving={leaving || undefined}
    >
      <span
        className={styles.hairline}
        aria-hidden="true"
        style={{ inlineSize: `${String(percent)}%` }}
      />
      <p ref={sentenceRef} role="status" className={styles.sentence} />
      {shown !== null && (
        <span className={styles.count}>
          {m.list_first_sync_count({ synced: shown.synced, total: shown.total }, { locale })}
        </span>
      )}
    </div>
  );
}
