// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import styles from "./kbd.module.css";

export function Kbd({ children }: { children: string }) {
  return <kbd className={styles.kbd}>{children}</kbd>;
}
