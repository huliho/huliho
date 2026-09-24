// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import styles from "./kbd.module.css";

interface KbdProps {
  children: string;
  // Whether a screen reader reads the key; a hint beside a spoken label stays silent.
  spoken?: boolean;
}

export function Kbd({ children, spoken = true }: KbdProps) {
  return (
    <kbd className={styles.kbd} aria-hidden={spoken ? undefined : true}>
      {children}
    </kbd>
  );
}
