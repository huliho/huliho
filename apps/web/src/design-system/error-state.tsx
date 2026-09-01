// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { CircleAlert } from "lucide-react";

import styles from "./error-state.module.css";

interface ErrorStateProps {
  message: string;
  retryLabel: string;
  onRetry: () => void;
}

export function ErrorState({ message, retryLabel, onRetry }: ErrorStateProps) {
  return (
    <div className={styles.screen} role="alert">
      <CircleAlert className={styles.icon} aria-hidden="true" />
      <p className={styles.message}>{message}</p>
      <button type="button" className={styles.retry} onClick={onRetry}>
        {retryLabel}
      </button>
    </div>
  );
}
