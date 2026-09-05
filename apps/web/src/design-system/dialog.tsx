// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Dialog as BaseDialog } from "@base-ui/react/dialog";
import type { ReactNode, RefObject } from "react";

import styles from "./dialog.module.css";

interface DialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  // Runs once the closing fade has ended, when what the dialog held can go.
  onClosed?: (() => void) | undefined;
  title: string;
  description?: string | undefined;
  // What went wrong with the dialog's own action, said above the content.
  failure?: string | undefined;
  // Focus lands here on open; the least destructive action goes first.
  initialFocus?: RefObject<HTMLElement | null> | undefined;
  children: ReactNode;
}

export function Dialog({
  open,
  onOpenChange,
  onClosed,
  title,
  description,
  failure,
  initialFocus,
  children,
}: DialogProps) {
  return (
    <BaseDialog.Root
      open={open}
      onOpenChange={onOpenChange}
      onOpenChangeComplete={(next) => {
        if (!next) {
          onClosed?.();
        }
      }}
    >
      <BaseDialog.Portal>
        <BaseDialog.Backdrop className={styles.backdrop} />
        <BaseDialog.Popup className={styles.popup} initialFocus={initialFocus ?? true}>
          <BaseDialog.Title className={styles.title}>{title}</BaseDialog.Title>
          {description !== undefined && (
            <BaseDialog.Description className={styles.description}>
              {description}
            </BaseDialog.Description>
          )}
          {failure !== undefined && (
            <p className={styles.failure} role="alert">
              {failure}
            </p>
          )}
          {children}
        </BaseDialog.Popup>
      </BaseDialog.Portal>
    </BaseDialog.Root>
  );
}

// The button row at the end of a dialog, least destructive action first.
export function DialogActions({ children }: { children: ReactNode }) {
  return <div className={styles.actions}>{children}</div>;
}
