// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useState } from "react";

import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { AttachmentChip } from "./attachment-chip";
import { PreviewDialog, WarningDialog } from "./attachment-dialogs";
import type { Opened } from "./attachment-dialogs";
import { AttachmentPreview } from "./attachment-preview";
import type { Attachment } from "./parts";
import styles from "./attachment-strip.module.css";

interface StripProps {
  locale: Locale;
  attachments: readonly Attachment[];
}

type Kind = "warning" | "preview";

// The dialog of the strip that stands or is closing, if any.
interface Shown extends Opened {
  kind: Kind;
}

// A part with a picture to show.
type Pictured = Attachment & { preview: string };

interface DialogsProps {
  locale: Locale;
  shown: Shown | null;
  open: boolean;
  onClose: () => void;
  onClosed: () => void;
}

// The two dialogs a part can open; one stands at a time.
function StripDialogs({ locale, shown, open, onClose, onClosed }: DialogsProps) {
  const of = (kind: Kind): Opened | null => (shown?.kind === kind ? shown : null);
  return (
    <>
      <WarningDialog
        locale={locale}
        opened={of("warning")}
        open={open}
        onClose={onClose}
        onClosed={onClosed}
      />
      <PreviewDialog
        locale={locale}
        opened={of("preview")}
        open={open}
        onClose={onClose}
        onClosed={onClosed}
      />
    </>
  );
}

// The strip under a body: a chip per part and the previews after them,
// each group in the message's order. A raster part previews until its
// answer turns out not to be that image; then it is a chip too. The
// dialog that a chip or a preview opens hands the focus back to it.
export function AttachmentStrip({ locale, attachments }: StripProps) {
  const [failed, setFailed] = useState<ReadonlySet<string>>(new Set());
  const [shown, setShown] = useState<Shown | null>(null);
  const [open, setOpen] = useState(false);
  const previews = attachments.filter(
    (one): one is Pictured => one.preview !== null && !failed.has(one.key),
  );
  const pictured = new Set(previews.map((one) => one.key));
  const chips = attachments.filter((one) => !pictured.has(one.key));
  const show =
    (kind: Kind) =>
    (attachment: Attachment, opener: HTMLElement): void => {
      setShown({ kind, attachment, opener });
      setOpen(true);
    };
  return (
    <div className={styles.strip}>
      <ul
        role="list"
        aria-label={m.attachment_list({ count: attachments.length }, { locale })}
        className={styles.list}
      >
        {chips.map((one) => (
          <li key={one.key} className={styles.item}>
            <AttachmentChip attachment={one} onWarn={show("warning")} />
          </li>
        ))}
        {previews.map((one) => (
          <li key={one.key} className={styles.item} data-preview>
            <AttachmentPreview
              attachment={one}
              preview={one.preview}
              onOpen={show("preview")}
              onFailed={(lost) => {
                setFailed((held) => new Set(held).add(lost.key));
              }}
            />
          </li>
        ))}
      </ul>
      <StripDialogs
        locale={locale}
        shown={shown}
        open={open}
        onClose={() => {
          setOpen(false);
        }}
        onClosed={() => {
          setShown(null);
        }}
      />
    </div>
  );
}
