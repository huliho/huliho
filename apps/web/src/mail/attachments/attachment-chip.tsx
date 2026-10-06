// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { File, FileArchive, FileText, FileVideo, Image, Mail } from "lucide-react";

import spoken from "../../design-system/spoken.module.css";
import { splitName } from "./parts";
import type { Attachment, Family } from "./parts";
import styles from "./attachment-strip.module.css";

const ICONS: Record<Family, typeof File> = {
  image: Image,
  document: FileText,
  archive: FileArchive,
  media: FileVideo,
  message: Mail,
  file: File,
};

// The name of a part: read out whole, drawn in two pieces so a long one
// gives way in the middle. It is mail content and reads in its own
// direction.
export function PartName({ label }: { label: string }) {
  const [start, end] = splitName(label);
  return (
    <span className={styles.name}>
      <span className={spoken.spoken}>{label}</span>
      <span aria-hidden="true" dir="auto" className={styles.pieces}>
        <span className={styles.start}>{start}</span>
        <span className={styles.end}>{end}</span>
      </span>
    </span>
  );
}

// The icon by family with the name and the size, as a chip reads
// inside either element. The space keeps the two apart when they are
// read out. The size reads its digits before its unit in any page
// direction.
function ChipFace({ attachment }: { attachment: Attachment }) {
  const Icon = ICONS[attachment.family];
  return (
    <>
      <Icon className={styles.icon} aria-hidden="true" />
      <PartName label={attachment.label} /> <bdi className={styles.size}>{attachment.size}</bdi>
    </>
  );
}

interface ChipProps {
  attachment: Attachment;
  // Told when a name that can run a program is clicked, with the chip.
  onWarn: (attachment: Attachment, opener: HTMLElement) => void;
}

// One attachment to take away. A safe name is a link that downloads;
// a name that can run a program is a button that asks first.
export function AttachmentChip({ attachment, onWarn }: ChipProps) {
  if (attachment.dangerous) {
    return (
      <button
        type="button"
        className={styles.chip}
        onClick={(event) => {
          onWarn(attachment, event.currentTarget);
        }}
      >
        <ChipFace attachment={attachment} />
      </button>
    );
  }
  return (
    <a className={styles.chip} href={attachment.download} download>
      <ChipFace attachment={attachment} />
    </a>
  );
}
