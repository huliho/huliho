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
  // Text, or an element where mail content has to read in its own
  // direction; never empty, so the dialog always has a name.
  title: NonNullable<ReactNode>;
  description?: ReactNode;
  // What went wrong with the dialog's own action, said above the content.
  failure?: string | undefined;
  // Focus lands here on open; the least destructive action goes first.
  initialFocus?: RefObject<HTMLElement | null> | undefined;
  // Where the focus goes when the dialog closes: the element that
  // opened it, for a dialog opened without a trigger of its own.
  finalFocus?: HTMLElement | undefined;
  // Controls beside the title, for a dialog whose content is a picture.
  header?: ReactNode;
  // The narrow measure for a question, the wide one for a table, the
  // image one for a picture at full size.
  size?: "narrow" | "wide" | "image" | undefined;
  children: ReactNode;
}

interface HeadingProps {
  title: NonNullable<ReactNode>;
  description: ReactNode;
}

function Heading({ title, description }: HeadingProps) {
  return (
    <>
      <BaseDialog.Title className={styles.title}>{title}</BaseDialog.Title>
      {description !== undefined && (
        <BaseDialog.Description className={styles.description}>
          {description}
        </BaseDialog.Description>
      )}
    </>
  );
}

export function Dialog({
  open,
  onOpenChange,
  onClosed,
  title,
  description,
  failure,
  initialFocus,
  finalFocus,
  header,
  size = "narrow",
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
        <BaseDialog.Popup
          className={styles.popup}
          data-size={size}
          initialFocus={initialFocus ?? true}
          finalFocus={finalFocus === undefined ? undefined : () => finalFocus}
        >
          {header === undefined ? (
            <Heading title={title} description={description} />
          ) : (
            <div className={styles.heading}>
              <div className={styles.words}>
                <Heading title={title} description={description} />
              </div>
              <div className={styles.tools}>{header}</div>
            </div>
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
