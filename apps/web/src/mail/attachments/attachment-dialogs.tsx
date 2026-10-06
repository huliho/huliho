// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { X } from "lucide-react";
import { useRef } from "react";

import { Button } from "../../design-system/button";
import buttonStyles from "../../design-system/button.module.css";
import { cx } from "../../design-system/cx";
import { Dialog, DialogActions } from "../../design-system/dialog";
import iconButton from "../../design-system/icon-button.module.css";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import type { Attachment } from "./parts";
import styles from "./attachment-strip.module.css";

// What a dialog of the strip stands for: the part and the element that
// opened it, which takes the focus back.
export interface Opened {
  attachment: Attachment;
  opener: HTMLElement;
}

interface DialogProps {
  locale: Locale;
  opened: Opened | null;
  // Open, or closing with its content still in place.
  open: boolean;
  onClose: () => void;
  // Runs once the closing fade has ended.
  onClosed: () => void;
}

// The question before a download that can run a program. Cancel has the
// focus, so Enter does the safe thing; Download anyway is a plain link.
// The file name is mail content and reads in its own direction, and so
// does the size.
export function WarningDialog({ locale, opened, open, onClose, onClosed }: DialogProps) {
  const cancel = useRef<HTMLButtonElement>(null);
  if (opened === null) {
    return null;
  }
  const { attachment, opener } = opened;
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          onClose();
        }
      }}
      onClosed={onClosed}
      title={m.attachment_dangerous({}, { locale })}
      description={
        <>
          <bdi>{attachment.label}</bdi> · <bdi>{attachment.size}</bdi>
        </>
      }
      initialFocus={cancel}
      finalFocus={opener}
    >
      <DialogActions>
        <Button ref={cancel} variant="primary" onClick={onClose}>
          {m.cancel_action({}, { locale })}
        </Button>
        <a
          className={cx(buttonStyles.button, buttonStyles.secondary)}
          href={attachment.download}
          download
          onClick={onClose}
        >
          {m.attachment_download_anyway({}, { locale })}
        </a>
      </DialogActions>
    </Dialog>
  );
}

// The image at full size, named and sized in its header, with Download
// and a way to close it. Escape closes it as well.
export function PreviewDialog({ locale, opened, open, onClose, onClosed }: DialogProps) {
  if (opened === null) {
    return null;
  }
  const { attachment, opener } = opened;
  const { preview } = attachment;
  if (preview === null) {
    return null;
  }
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          onClose();
        }
      }}
      onClosed={onClosed}
      title={<bdi>{attachment.label}</bdi>}
      description={<bdi>{attachment.size}</bdi>}
      size="image"
      finalFocus={opener}
      header={
        <>
          <a
            className={cx(buttonStyles.button, buttonStyles.secondary)}
            href={attachment.download}
            download
          >
            {m.attachment_download({}, { locale })}
          </a>
          <button
            type="button"
            className={iconButton.button}
            aria-label={m.attachment_close({}, { locale })}
            onClick={onClose}
          >
            <X className={iconButton.icon} aria-hidden="true" />
          </button>
        </>
      }
    >
      <img className={styles.full} src={preview} alt={attachment.label} />
    </Dialog>
  );
}
