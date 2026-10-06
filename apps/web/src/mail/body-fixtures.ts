// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type {
  Authentication,
  BodyDetail,
  EmailBody,
  EmailBodyPart,
  EmailBodyValue,
  EmailHeader,
} from "@huliho/core";

import { DOWNLOAD, part } from "./body/frame-rig";
import { WHOLE_MESSAGE_NAME } from "./body-shape";

// The server that stamped the fixture messages, as its header names it.
export const AUTHSERV = "mx.fastmail.example";

// A message that passed the receiving server's check for Pieter's domain.
export const PASSED: Authentication = {
  status: "parsed",
  results: {
    server: AUTHSERV,
    spf: "pass",
    dkim: "pass",
    dmarc: "pass",
    dmarcFrom: "blom-installaties.example",
  },
};

// A message that failed it.
export const FAILED: Authentication = {
  status: "parsed",
  results: { server: AUTHSERV, spf: "pass", dkim: "fail", dmarc: "fail", dmarcFrom: null },
};

function valueOf(value: string, cut: boolean): EmailBodyValue {
  return { value, isEncodingProblem: false, isTruncated: cut };
}

interface BodyFacts {
  authentication?: Authentication;
  attachments?: EmailBodyPart[];
  flowed?: { delSp: boolean } | null;
  // Whether the value was cut at the cap it was asked at.
  cut?: boolean;
  // Whether the values were asked at the large cap.
  large?: boolean;
}

function body(
  id: string,
  facts: BodyFacts,
): Omit<EmailBody, "bodyStructure" | "textBody" | "htmlBody" | "bodyValues"> {
  return {
    id,
    attachments: facts.attachments ?? [],
    authentication: facts.authentication ?? { status: "absent" },
    flowed: facts.flowed ?? null,
    large: facts.large ?? false,
    fetchedAt: 0,
    bytes: 0,
  };
}

// A message of one text part.
export function textDetail(id: string, text: string, facts: BodyFacts = {}): BodyDetail {
  const textPart = part("text/plain", { partId: "1", blobId: `${id}-1`, size: text.length });
  return {
    body: {
      ...body(id, facts),
      bodyStructure: textPart,
      textBody: [textPart],
      htmlBody: [textPart],
      bodyValues: { "1": valueOf(text, facts.cut ?? false) },
    },
    download: DOWNLOAD,
  };
}

// A message with a text and an HTML alternative.
export function htmlDetail(id: string, html: string, facts: BodyFacts = {}): BodyDetail {
  const textPart = part("text/plain", { partId: "1", blobId: `${id}-1` });
  const htmlPart = part("text/html", { partId: "2", blobId: `${id}-2`, size: html.length });
  return {
    body: {
      ...body(id, facts),
      bodyStructure: part("multipart/alternative"),
      textBody: [textPart],
      htmlBody: [htmlPart],
      bodyValues: {
        "1": valueOf(html.replaceAll(/<[^>]+>/g, ""), false),
        "2": valueOf(html, facts.cut ?? false),
      },
    },
    download: DOWNLOAD,
  };
}

// A message without text or HTML: one attachment alone.
export function attachmentDetail(id: string): BodyDetail {
  const pdf = part("application/pdf", { blobId: `${id}-1`, name: "offerte.pdf", size: 48_210 });
  return {
    body: {
      ...body(id, { attachments: [pdf] }),
      bodyStructure: pdf,
      textBody: [],
      htmlBody: [],
      bodyValues: {},
    },
    download: DOWNLOAD,
  };
}

// A photo among the attachments, which previews in the strip.
export const PHOTO_PART = part("image/jpeg", {
  partId: "7",
  blobId: "b-photo",
  name: "tegelwerk_voorbeeld.jpg",
  size: 2_400_000,
});

// The attachments of a message as a sender sends them: a long name, an
// unnamed part, an attached message, the photo, two files a download
// has to ask about first and an image whose name asks first as well.
// Their ids follow the text and HTML parts of the messages they ride.
export const ATTACHMENT_PARTS: EmailBodyPart[] = [
  part("application/pdf", {
    partId: "4",
    blobId: "b-offerte",
    name: "Offerte_badkamer_renovatie_v3_definitief.pdf",
    size: 48_210,
  }),
  part("application/octet-stream", { partId: "5", blobId: "b-unnamed", size: 1024 }),
  part("message/rfc822", { partId: "6", blobId: "b-forwarded", size: 12_400 }),
  PHOTO_PART,
  part("image/svg+xml", { partId: "8", blobId: "b-diagram", name: "diagram.svg", size: 8192 }),
  part("text/html", {
    partId: "9",
    blobId: "b-viewer",
    name: "factuur_viewer.html",
    size: 36_000,
  }),
  part("image/png", { partId: "10", blobId: "b-kaart", name: "kaart.html", size: 20_000 }),
];

// A plain message with the attachments given, all of them by default.
export const ATTACHMENTS_TEXT = "In de bijlage de offerte en een foto van het tegelwerk.";

export function attachmentsDetail(id: string, parts = ATTACHMENT_PARTS): BodyDetail {
  return textDetail(id, ATTACHMENTS_TEXT, { attachments: parts });
}

// A message the server could not describe: one part over the whole of it.
export function complexDetail(id: string): BodyDetail {
  const whole = part("application/octet-stream", {
    blobId: id,
    name: WHOLE_MESSAGE_NAME,
    size: 2_400_000,
  });
  return {
    body: {
      ...body(id, { attachments: [whole] }),
      bodyStructure: whole,
      textBody: [],
      htmlBody: [],
      bodyValues: {},
    },
    download: DOWNLOAD,
  };
}

// The body a header stands in for until the message has one of its own:
// its preview sentence as a text part.
export function previewDetail(header: EmailHeader): BodyDetail {
  return textDetail(header.id, header.preview);
}

// A newsletter: a remote header image, a column of text and a footer
// with a link.
export const NEWSLETTER_HTML = [
  '<div style="background: #f4efe6; padding: 24px; font-family: Georgia, serif; color: #2b2622">',
  '<img src="https://cdn.example/hero.png" width="552" height="180" alt="Autumn sale">',
  '<h1 style="font-size: 24px; margin: 24px 0 8px">Week 35: rentes, chips en de bouw</h1>',
  '<p style="line-height: 1.5">Deze week houdt de ECB vast, knelt de chipexport en vindt de bouw ',
  "zijn vakmensen niet. Drie stukken, één grafiek en de agenda voor september.</p>",
  '<p><a href="https://shop.example.test/sale" style="color: #1e7688">Lees verder</a></p>',
  '<p style="font-size: 12px; color: #676d6f">De Koersbrief, Keizersgracht 1, Amsterdam</p>',
  "</div>",
].join("");

const LOGO_CID = "logo@koersbrief.example";

// The sender's logo as a part of the message, with the image that names it.
export const LOGO = part("image/png", {
  partId: "3",
  blobId: "e-3-3",
  name: "logo.png",
  cid: LOGO_CID,
  size: 1860,
});
export const LOGO_IMAGE = `<img src="cid:${LOGO_CID}" width="24" height="24" alt="De Koersbrief">`;

// A reply with a two-deep quote, as a plain mail carries it.
export const REPLY_TEXT = [
  "Dank Pieter, kan het tegelwerk in dezelfde prijs mee?",
  "",
  "Zie https://www.blom-installaties.example/offertes voor de voorwaarden.",
  "",
  "Op 12 mei schreef Pieter Blom:",
  "> Hierbij de eerste versie van de offerte voor de badkamer.",
  "> De meerprijs voor de vloerverwarming ontbreekt nog.",
  ">",
  "> Op 11 mei schreef Sanne Bakker:",
  ">> Kunnen jullie een offerte maken voor de badkamer?",
  ">> Groet, Sanne",
].join("\n");
