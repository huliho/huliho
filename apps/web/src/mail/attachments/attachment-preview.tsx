// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { PartName } from "./attachment-chip";
import type { Attachment } from "./parts";
import styles from "./attachment-strip.module.css";

interface PreviewProps {
  attachment: Attachment;
  // The route with the declared type.
  preview: string;
  onOpen: (attachment: Attachment, opener: HTMLElement) => void;
  // Told when the answer was not that image, so the part shows as a chip.
  onFailed: (attachment: Attachment) => void;
}

// A raster part in place, with its name and size under it; a click
// opens it at full size. The caption names the image, so the picture
// itself says nothing twice. A picture loads when it comes into view,
// so a message of many photos asks the route for the ones in sight.
export function AttachmentPreview({ attachment, preview, onOpen, onFailed }: PreviewProps) {
  return (
    <button
      type="button"
      className={styles.preview}
      onClick={(event) => {
        onOpen(attachment, event.currentTarget);
      }}
    >
      <img
        className={styles.picture}
        src={preview}
        alt=""
        loading="lazy"
        decoding="async"
        onError={() => {
          onFailed(attachment);
        }}
      />
      <span className={styles.caption}>
        <PartName label={attachment.label} /> <bdi className={styles.size}>{attachment.size}</bdi>
      </span>
    </button>
  );
}
