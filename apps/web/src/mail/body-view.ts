// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { BODY_VALUE_BYTES, BODY_VALUE_BYTES_LARGE, downloadUrl } from "@huliho/core";
import type { BodyDetail, EmailBody, EmailHeader } from "@huliho/core";

import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { WHOLE_MESSAGE_NAME, isCut } from "./body-shape";
import type { Cut } from "./message-body";
import { senderOf } from "./recipients";

const MIB = 1024 * 1024;

// The whole message downloads under the email's own blob id, as a file.
const MESSAGE_TYPE = "message/rfc822";

// The size a cut body was asked at, as the sentence shows it.
export function cutSize(body: EmailBody, locale: Locale): string {
  const bytes = body.large ? BODY_VALUE_BYTES_LARGE : BODY_VALUE_BYTES;
  return new Intl.NumberFormat(locale, {
    style: "unit",
    unit: "megabyte",
    maximumFractionDigits: 0,
  }).format(bytes / MIB);
}

// Where the whole message downloads from.
export function messageDownload(detail: BodyDetail, email: EmailHeader): string {
  return downloadUrl(detail.download, {
    blobId: email.blobId,
    name: WHOLE_MESSAGE_NAME,
    type: MESSAGE_TYPE,
  });
}

interface WholeAsk {
  show: () => void;
  pending: boolean;
}

// What a cut body offers: the second ask until it was made, the
// download once that ask came back cut as well; null for a whole body.
export function cutOf(
  detail: BodyDetail,
  email: EmailHeader,
  locale: Locale,
  whole: WholeAsk,
): Cut | null {
  if (!isCut(detail.body)) {
    return null;
  }
  const asked = detail.body.large;
  return {
    kind: "size",
    size: cutSize(detail.body, locale),
    whole: asked ? null : whole.show,
    pending: whole.pending,
    download: asked ? messageDownload(detail, email) : null,
  };
}

// The frame's accessible name: who wrote and what about.
export function frameTitle(email: EmailHeader, locale: Locale): string {
  const sender = senderOf(email).name ?? m.list_no_sender({}, { locale });
  const subject = email.subject?.trim() ?? "";
  return subject === ""
    ? m.body_frame_title_no_subject({ sender }, { locale })
    : m.body_frame_title({ sender, subject }, { locale });
}
