// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import styles from "./empty-state.module.css";

interface EmptyStateProps {
  message: string;
}

// The view with nothing in it yet: one sentence, with the next thing to
// do placed under it by the page.
export function EmptyState({ message }: EmptyStateProps) {
  return (
    <div className={styles.state}>
      <p className={styles.message}>{message}</p>
    </div>
  );
}
