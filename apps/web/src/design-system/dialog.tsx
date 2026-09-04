// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Dialog as BaseDialog } from "@base-ui/react/dialog";
import type { ReactNode, RefObject } from "react";

import styles from "./dialog.module.css";

interface DialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: string | undefined;
  // Focus lands here on open; the least destructive action goes first.
  initialFocus?: RefObject<HTMLElement | null> | undefined;
  children: ReactNode;
}

export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  initialFocus,
  children,
}: DialogProps) {
  return (
    <BaseDialog.Root open={open} onOpenChange={onOpenChange}>
      <BaseDialog.Portal>
        <BaseDialog.Backdrop className={styles.backdrop} />
        <BaseDialog.Popup className={styles.popup} initialFocus={initialFocus ?? true}>
          <BaseDialog.Title className={styles.title}>{title}</BaseDialog.Title>
          {description !== undefined && (
            <BaseDialog.Description className={styles.description}>
              {description}
            </BaseDialog.Description>
          )}
          <div className={styles.actions}>{children}</div>
        </BaseDialog.Popup>
      </BaseDialog.Portal>
    </BaseDialog.Root>
  );
}
