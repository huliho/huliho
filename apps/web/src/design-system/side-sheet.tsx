// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Dialog } from "@base-ui/react/dialog";
import { X } from "lucide-react";
import type { ReactNode } from "react";

import styles from "./side-sheet.module.css";

interface SideSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  // What the panel holds, for anyone who cannot see it.
  label: string;
  closeLabel: string;
  children: ReactNode;
}

// A panel that slides in over the page from the inline start. Its Close
// button, Escape or a press outside closes it.
export function SideSheet({ open, onOpenChange, label, closeLabel, children }: SideSheetProps) {
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Backdrop className={styles.backdrop} />
        <Dialog.Popup className={styles.popup} aria-label={label}>
          <div className={styles.bar}>
            <Dialog.Close className={styles.close} aria-label={closeLabel}>
              <X className={styles.icon} aria-hidden="true" />
            </Dialog.Close>
          </div>
          {children}
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
