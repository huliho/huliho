// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The bodies behind the corpus messages, one shape per sender, so a
// test finds a newsletter, a quoted reply or a cut message by who wrote
// it; and the body properties of an Email/get answer built from one.

import { NOT_AN_IMAGE_NAME } from "./blob-mocks";
import type { CorpusEmail } from "./mail-corpus";

// One part with the default body properties (RFC 8621 section 4.1.4).
interface CorpusPart {
  partId: string | null;
  blobId: string | null;
  size: number;
  name: string | null;
  type: string;
  charset: string | null;
  disposition: string | null;
  cid: string | null;
  language: string[] | null;
  location: string | null;
}

export interface CorpusBody {
  text?: string;
  html?: string;
  attachments?: CorpusPart[];
  // Every Authentication-Results header, the topmost first.
  authenticationResults?: string[];
  contentType?: string;
  // Whether the values come back cut: at the first ask, or at the ask
  // at the large cap as well. The values stay short; the card reads
  // the flag alone, as it would for a server that cut for its own
  // reasons.
  cut?: "once" | "twice";
}

interface BodyArgs {
  fetchTextBodyValues?: boolean;
  fetchHTMLBodyValues?: boolean;
  maxBodyValueBytes?: number;
}

// The cap of the first ask; a larger cap is the second ask.
const FIRST_CAP_BYTES = 4 * 1024 * 1024;

// The receiving server behind the mocked proxy, as its header names it.
export const AUTHSERV = "mx.example.net";

// The addresses a newsletter loads from: the remote image and the link.
export const HERO_URL = "https://cdn.example/hero.png";
export const SHOP_URL = "https://shop.example.test/sale";
const LOGO_CID = "logo@koersbrief.example";

// Who writes which body.
export const NEWSLETTER_SENDER = "redactie@koersbrief.example";
export const REPLY_SENDER = "pieter@blom-installaties.example";
export const MICROSOFT_SENDER = "nieuws@kastanje.example";
export const FAILED_SENDER = "anouk@kastanje.example";
export const CUT_ONCE_SENDER = "tomas@lindqvist.example";
export const CUT_TWICE_SENDER = "jeroen@vos-advies.example";
export const NO_TEXT_SENDER = "iris@familie.example";
export const ATTACHMENTS_SENDER = "jonas@kastanje.example";
// A header the server left with two of its methods unchecked and one it
// could not settle, and one no reader can make sense of.
export const UNCHECKED_SENDER = "femke@kastanje.example";
export const UNREADABLE_SENDER = "sven@hosting.example";

// The attachments of the planning mail: the names a test looks for.
export const LONG_NAME = "Offerte_badkamer_renovatie_v3_definitief.pdf";
export const PHOTO_NAME = "tegelwerk_voorbeeld.png";
export const SVG_NAME = "diagram.svg";
export const DANGEROUS_NAME = "factuur_viewer.html";
// An image whose name asks first, whatever its bytes are.
export const DANGEROUS_IMAGE_NAME = "kaart.html";
const ATTACHMENTS_TEXT = "De deck is leidend; de budgetsheet en een foto van het bord staan erbij.";

// The receiving server's header for a message that passed every check
// and for one that failed.
export function passedBy(domain: string): string {
  return ` ${AUTHSERV}; spf=pass smtp.mailfrom=${domain}; dkim=pass header.d=${domain}; dmarc=pass header.from=${domain}`;
}

export function failedBy(domain: string): string {
  return ` ${AUTHSERV}; spf=pass smtp.mailfrom=${domain}; dkim=fail header.d=${domain}; dmarc=fail header.from=${domain}`;
}

// The shape a Microsoft server writes: no server name in front.
const MICROSOFT_HEADER = [
  " spf=pass (sender IP is 203.0.113.5)",
  " smtp.mailfrom=kastanje.example; dkim=pass (signature was verified)",
  " header.d=kastanje.example;dmarc=pass action=none",
  " header.from=kastanje.example;compauth=pass reason=100",
].join("\r\n");

// A server that checked nothing of SPF and DMARC and found DKIM neither way.
const UNCHECKED_HEADER = ` ${AUTHSERV}; spf=none smtp.mailfrom=kastanje.example; dkim=neutral header.d=kastanje.example; dmarc=none header.from=kastanje.example`;

// Two DMARC clauses in one header: a value the sender chose, copied unquoted.
const UNREADABLE_HEADER = ` ${AUTHSERV}; dmarc=pass header.from=hosting.example; dmarc=fail header.from=other.example`;

export const NEWSLETTER_HTML = [
  '<div style="background: #f4efe6; padding: 24px; font-family: Georgia, serif; color: #2b2622">',
  `<img src="${HERO_URL}" width="552" height="180" alt="Autumn sale">`,
  '<h1 style="font-size: 24px; margin: 24px 0 8px">Week 35: rentes, chips en de bouw</h1>',
  '<p style="line-height: 1.5">Deze week houdt de ECB vast, knelt de chipexport en vindt de bouw ',
  "zijn vakmensen niet. Drie stukken, één grafiek en de agenda voor september.</p>",
  `<p><a href="${SHOP_URL}" style="color: #1e7688">Lees verder</a></p>`,
  `<p style="font-size: 12px; color: #676d6f"><img src="cid:${LOGO_CID}" `,
  'width="24" height="24" alt="De Koersbrief"> De Koersbrief, Keizersgracht 1, Amsterdam</p>',
  "</div>",
].join("");

// A short mail with one remote image, for the senders whose header is the point.
function pictured(words: string, host: string): string {
  return `<p>${words}</p><img src="https://${host}/banner.png" width="400" height="80" alt="Banner">`;
}

const REPLY_TEXT = [
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

const FLOWED_TYPE = " text/plain; charset=utf-8; format=flowed";

function part(type: string, facts: Partial<CorpusPart> = {}): CorpusPart {
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

// The sender's logo, carried by the message itself.
function logoPart(emailId: string): CorpusPart {
  return part("image/png", {
    partId: "3",
    blobId: `${emailId}-3`,
    size: 1860,
    name: "logo.png",
    disposition: "inline",
    cid: LOGO_CID,
  });
}

// A long name, an unnamed part, an attached message, a photo, an SVG, a
// file that can run a program, a part declared an image whose bytes are
// not one and an image whose name asks first.
function attachmentParts(emailId: string): CorpusPart[] {
  const attached = (partId: string, type: string, name: string | null, size: number) =>
    part(type, { partId, blobId: `${emailId}-${partId}`, name, size, disposition: "attachment" });
  return [
    attached("2", "application/pdf", LONG_NAME, 48_210),
    attached("3", "application/octet-stream", null, 1024),
    attached("4", "message/rfc822", null, 12_400),
    attached("5", "image/png", PHOTO_NAME, 2_400_000),
    attached("6", "image/svg+xml", SVG_NAME, 8192),
    attached("7", "text/html", DANGEROUS_NAME, 36_000),
    attached("8", "image/png", NOT_AN_IMAGE_NAME, 512_000),
    attached("9", "image/png", DANGEROUS_IMAGE_NAME, 20_000),
  ];
}

const BY_SENDER = new Map<string, (message: CorpusEmail) => CorpusBody>([
  [
    NEWSLETTER_SENDER,
    (message) => ({
      html: NEWSLETTER_HTML,
      attachments: [logoPart(message.id)],
      authenticationResults: [passedBy("koersbrief.example")],
    }),
  ],
  [REPLY_SENDER, () => ({ text: REPLY_TEXT, contentType: FLOWED_TYPE })],
  [
    MICROSOFT_SENDER,
    () => ({
      html: pictured("Drie nieuwe projecten en een verhuizing.", "img.kastanje.example"),
      authenticationResults: [MICROSOFT_HEADER],
    }),
  ],
  [
    FAILED_SENDER,
    () => ({
      html: pictured("Het concept staat klaar, graag je reactie.", "img.kastanje.example"),
      authenticationResults: [failedBy("kastanje.example")],
    }),
  ],
  [
    CUT_ONCE_SENDER,
    (message) => ({ text: `${message.preview}\n\nThe first part of a long one.`, cut: "once" }),
  ],
  [CUT_TWICE_SENDER, (message) => ({ html: `<p>${message.preview}</p>`, cut: "twice" })],
  [
    NO_TEXT_SENDER,
    (message) => ({
      attachments: [
        part("application/pdf", { blobId: `${message.id}-1`, name: "taart.pdf", size: 48_210 }),
      ],
    }),
  ],
  [
    ATTACHMENTS_SENDER,
    (message) => ({ text: ATTACHMENTS_TEXT, attachments: attachmentParts(message.id) }),
  ],
  [
    UNCHECKED_SENDER,
    (message) => ({ text: message.preview, authenticationResults: [UNCHECKED_HEADER] }),
  ],
  [
    UNREADABLE_SENDER,
    (message) => ({ text: message.preview, authenticationResults: [UNREADABLE_HEADER] }),
  ],
]);

// The body a message carries: the one a test set, else its sender's
// shape, else its preview as plain text.
export function bodyOf(message: CorpusEmail, bodies: ReadonlyMap<string, CorpusBody>): CorpusBody {
  return (
    bodies.get(message.id) ??
    BY_SENDER.get(message.from.email)?.(message) ?? {
      text: message.preview,
    }
  );
}

interface Leaf {
  partId: string;
  part: CorpusPart;
  value: string;
}

function leaf(emailId: string, partId: string, type: string, value: string | undefined): Leaf[] {
  if (value === undefined) {
    return [];
  }
  const one = part(type, {
    partId,
    blobId: `${emailId}-${partId}`,
    size: value.length,
    charset: "utf-8",
  });
  return [{ partId, part: one, value }];
}

// The body lists of one email: the text part and the HTML part, each
// standing in for the other where one is missing (RFC 8621 section 4.1.4).
function alternativesOf(message: CorpusEmail, body: CorpusBody) {
  const text = leaf(message.id, "1", "text/plain", body.text);
  const html = leaf(message.id, "2", "text/html", body.html);
  const [first, second] = [...text, ...html];
  if (first === undefined) {
    return { root: part("multipart/mixed"), text, html };
  }
  if (second === undefined) {
    return { root: first.part, text: [first], html: [first] };
  }
  return { root: part("multipart/alternative"), text, html };
}

// The body properties of one email, as the proxy would answer them.
export function bodyPropertiesOf(
  message: CorpusEmail,
  body: CorpusBody,
  args: BodyArgs,
): Record<string, unknown> {
  const { root, text, html } = alternativesOf(message, body);
  const asked = [
    ...(args.fetchTextBodyValues === true ? text : []),
    ...(args.fetchHTMLBodyValues === true ? html : []),
  ];
  const large = (args.maxBodyValueBytes ?? FIRST_CAP_BYTES) > FIRST_CAP_BYTES;
  const isTruncated = body.cut === "twice" || (body.cut === "once" && !large);
  return {
    bodyStructure: root,
    textBody: text.map((one) => one.part),
    htmlBody: html.map((one) => one.part),
    attachments: body.attachments ?? [],
    bodyValues: Object.fromEntries(
      asked.map(({ partId, value }) => [partId, { value, isEncodingProblem: false, isTruncated }]),
    ),
    "header:Authentication-Results:asRaw:all": body.authenticationResults ?? [],
    "header:Content-Type:asRaw": body.contentType ?? ` ${root.type}; charset=utf-8`,
  };
}
