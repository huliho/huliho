// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { FirstSync } from "@huliho/core";
import { useEffect, useRef, useState } from "react";

import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { useStatusText } from "./status-text";
import styles from "./first-sync.module.css";

// How long the block stays while it fades out, the duration of one state flip.
const LEAVE_MS = 120;
const PERCENT = 100;

interface FirstSyncBlockProps {
  locale: Locale;
  // The sync's progress, null once every header is in.
  progress: FirstSync | null;
}

// The progress last shown, kept for the fade once the sync is over.
function useLingering(progress: FirstSync | null): { shown: FirstSync | null; leaving: boolean } {
  const [held, setHeld] = useState(progress);
  if (progress !== null && progress !== held) {
    setHeld(progress);
  }
  useEffect(() => {
    if (progress !== null) {
      return undefined;
    }
    const timer = setTimeout(() => {
      setHeld(null);
    }, LEAVE_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [progress]);
  return { shown: progress ?? held, leaving: progress === null && held !== null };
}

// The foot of the list while the first header sync runs: the sentence,
// the count and a hairline at synced over total. The status region is
// always in the DOM and the sentence lands in it once the block renders.
export function FirstSyncBlock({ locale, progress }: FirstSyncBlockProps) {
  const { shown, leaving } = useLingering(progress);
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
