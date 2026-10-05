// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailBody, EmailBodyPart } from "@huliho/core";

import type { FrameStyle } from "./base-style";
import { buildFrameDocument } from "./frame-document";
import type { FrameDocument, FrameOptions } from "./frame-document";

// The instance the tests run on, so the own-host cases bite.
export const OWN_HOST = "mail.example.test";

export const DOWNLOAD = {
  template: "/api/jmap/acc-1/download/{accountId}/{blobId}/{name}?type={type}",
  accountId: "u1",
};

// Where every part of the message downloads from.
export const DOWNLOAD_PREFIX = "/api/jmap/acc-1/download/u1/";

export const PROXY_PREFIX = "/api/remote-image?url=";

const STYLE: FrameStyle = {
  fontFamily: "system-ui, sans-serif",
  fontSize: "13px",
  lineHeight: "1.45",
  bg: "rgb(242, 245, 246)",
  surface: "rgb(255, 255, 255)",
  border: "rgb(223, 230, 232)",
  text: "rgb(35, 43, 47)",
  muted: "rgb(103, 109, 111)",
};

export const OPTIONS: FrameOptions = {
  theme: "light",
  adapt: false,
  remote: false,
  ownHost: OWN_HOST,
  download: DOWNLOAD,
  linkKey: "key-1",
  style: STYLE,
};

export function part(type: string, facts: Partial<EmailBodyPart> = {}): EmailBodyPart {
  return {
    partId: null,
    blobId: null,
    size: 0,
    name: null,
    type,
    charset: null,
    disposition: null,
    cid: null,
    language: null,
    location: null,
    ...facts,
  };
}

// A message whose body list holds the given parts; a string stands for
// an HTML part with that value.
export function message(
  parts: readonly (string | EmailBodyPart)[],
  attachments: readonly EmailBodyPart[] = [],
): EmailBody {
  const values: [string, string][] = [];
  const htmlBody = parts.map((entry, index) => {
    if (typeof entry !== "string") {
      return entry;
    }
    const partId = String(index + 1);
    values.push([partId, entry]);
    return part("text/html", { partId });
  });
  return {
    id: "e1",
    bodyStructure: part("multipart/mixed"),
    textBody: [],
    htmlBody,
    attachments: [...attachments],
    bodyValues: Object.fromEntries(
      values.map(([partId, value]) => [
        partId,
        { value, isEncodingProblem: false, isTruncated: false },
      ]),
    ),
    authentication: { status: "absent" },
    flowed: null,
    large: false,
    fetchedAt: 0,
    bytes: 0,
  };
}

// The frame's document for one HTML value, built and read back as the
// frame would parse it.
export function built(
  html: string,
  options: Partial<FrameOptions> = {},
): FrameDocument & { page: Document } {
  const frame = buildFrameDocument(message([html]), { ...OPTIONS, ...options });
  return { ...frame, page: new DOMParser().parseFromString(frame.html, "text/html") };
}
