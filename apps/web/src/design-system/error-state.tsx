// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { CircleAlert } from "lucide-react";

import { Button } from "./button";
import styles from "./error-state.module.css";

interface ErrorStateProps {
  message: string;
  retryLabel: string;
  onRetry: () => void;
}

export function ErrorState({ message, retryLabel, onRetry }: ErrorStateProps) {
  return (
    <div className={styles.state} role="alert">
      <CircleAlert className={styles.icon} aria-hidden="true" />
      <p className={styles.message}>{message}</p>
      <Button onClick={onRetry}>{retryLabel}</Button>
    </div>
  );
}
