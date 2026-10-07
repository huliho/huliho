// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Skeleton } from "./skeleton";
import styles from "./text-lines.module.css";

// The still lines that stand for a text on its way.
const LINES = 3;

interface TextLinesProps {
  // What a screen reader hears while the lines stand.
  label: string;
}

export function TextLines({ label }: TextLinesProps) {
  return (
    <div role="status" aria-label={label} className={styles.lines}>
      {Array.from({ length: LINES }, (_, index) => (
        <Skeleton key={index} className={styles.line} />
      ))}
    </div>
  );
}
