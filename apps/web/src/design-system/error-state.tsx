// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { CircleAlert } from "lucide-react";

import { LeavingButton } from "./button";
import styles from "./error-state.module.css";

interface ErrorStateProps {
  message: string;
  retryLabel: string;
  onRetry: () => void;
  // A pane shows the state centered in its room; a card shows it as one
  // line at its own padding.
  variant?: "pane" | "inline";
}

// The retry takes the state away, so its button hands the focus on.
export function ErrorState({ message, retryLabel, onRetry, variant = "pane" }: ErrorStateProps) {
  return (
    <div className={styles.state} role="alert" data-variant={variant}>
      <CircleAlert className={styles.icon} aria-hidden="true" />
      <p className={styles.message}>{message}</p>
      <LeavingButton onClick={onRetry}>{retryLabel}</LeavingButton>
    </div>
  );
}
