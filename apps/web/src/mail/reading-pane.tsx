// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import styles from "./reading-pane.module.css";

// The third pane with nothing open in it.
export function ReadingPane({ locale }: { locale: Locale }) {
  return (
    <aside className={styles.pane} aria-label={m.thread_pane({}, { locale })}>
      <p className={styles.nothingOpen}>{m.thread_none_open({}, { locale })}</p>
    </aside>
  );
}
