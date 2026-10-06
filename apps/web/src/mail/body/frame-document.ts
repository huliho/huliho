// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { DownloadSource, EmailBody, EmailBodyPart } from "@huliho/core";

import type { Theme } from "../../theme/use-theme";
import { baseStyle } from "./base-style";
import type { Canvas, FrameStyle } from "./base-style";
import { cssContext } from "./css";
import type { CssContext } from "./css";
import { Images } from "./images";
import { rewriteLinks } from "./links";
import { adaptPiece, cleanPiece, paintsBackground, pieceHtml } from "./passes";
import type { Piece } from "./passes";
import { purify } from "./profile";

// The sandbox of the frame a mail renders in. Nothing runs inside it;
// the host reads its height and its links open a tab of their own.
export const FRAME_SANDBOX = "allow-same-origin allow-popups allow-popups-to-escape-sandbox";

// The frame's own policy, inside the app's: images from this instance
// and from the mail itself, the mail's inline styles and nothing else.
export const FRAME_CSP =
  "default-src 'none'; img-src 'self' data:; style-src 'unsafe-inline'; " +
  "base-uri 'none'; form-action 'none'";

const HEAD = [
  '<meta charset="utf-8">',
  `<meta http-equiv="Content-Security-Policy" content="${FRAME_CSP}">`,
  '<meta name="referrer" content="no-referrer">',
  '<meta http-equiv="x-dns-prefetch-control" content="off">',
].join("");

const HTML_TYPE = "text/html";
const IMAGE_TYPE = "image/";
const TEXT_TYPE = "text/";

export interface FrameOptions {
  // The theme the app renders.
  theme: Theme;
  // Whether a light-only mail is adapted in the dark theme: the
  // reader's setting says so and the engine can.
  adapt: boolean;
  // Whether the sender's remote images load, through the proxy.
  remote: boolean;
  // The host name the app runs on.
  ownHost: string;
  // Where the parts of the message download from.
  download: DownloadSource;
  // The device's key, which the open route asks of a link.
  linkKey: string;
  // What the frame takes from the card around it.
  style: FrameStyle;
}

export interface FrameDocument {
  // The document for the frame's srcdoc.
  html: string;
  // The remote URLs found, loaded or not.
  remote: number;
  // Whether the mail brings styles of its own for the dark scheme.
  declaresDark: boolean;
  // Whether the colors were adapted, so a revert has something to undo.
  adapted: boolean;
  // The ids of the parts the document shows inline.
  inlineParts: ReadonlySet<string>;
}

// A document that runs and loads nothing, for the parts built here.
function inert(): Document {
  return document.implementation.createHTMLDocument("");
}

// A plain text part among the HTML ones, as text and never as markup.
function textPiece(text: string): HTMLElement {
  const { body } = inert();
  const block = body.appendChild(body.ownerDocument.createElement("div"));
  block.setAttribute("style", "white-space: pre-wrap");
  block.textContent = text;
  return body;
}

// An image part of the body list, from the download route.
function imagePiece(part: EmailBodyPart, context: CssContext): HTMLElement {
  const { body } = inert();
  const placed = context.images.part(part);
  if (placed.kind === "loads") {
    const image = body.appendChild(body.ownerDocument.createElement("img"));
    image.setAttribute("src", placed.url);
    image.setAttribute("alt", part.name ?? "");
  }
  return body;
}

// One part of the body list: an HTML part through the sanitizer and
// the passes; an image and any other text as built here, which the
// passes have nothing to read in. A part of another kind renders
// nothing here and neither does one without a value.
function pieceOf(
  part: EmailBodyPart,
  values: ReadonlyMap<string, string>,
  context: CssContext,
  options: FrameOptions,
): Piece | null {
  const type = part.type.toLowerCase();
  if (type.startsWith(IMAGE_TYPE)) {
    return { root: imagePiece(part, context), sheets: [] };
  }
  const value = part.partId === null ? undefined : values.get(part.partId);
  if (value === undefined || !type.startsWith(TEXT_TYPE)) {
    return null;
  }
  if (type !== HTML_TYPE) {
    return { root: textPiece(value), sheets: [] };
  }
  const root = purify(value);
  rewriteLinks(root, options);
  return cleanPiece(root, context, options.style);
}

function canvasOf(theme: Theme, reads: boolean): Canvas {
  if (theme === "light") {
    return "light";
  }
  return reads ? "dark" : "as-sent";
}

// The document a mail renders from: every part of its HTML body in
// order, sanitized once more, its CSS cleaned and its URLs settled by
// the policy, its links pointed at the open route, under the frame's
// policy and base style. It is rebuilt for every view and never stored.
export function buildFrameDocument(body: EmailBody, options: FrameOptions): FrameDocument {
  const images = new Images(body, options);
  const context = cssContext(images, options.theme);
  const values = new Map(
    Object.entries(body.bodyValues).map(([partId, part]) => [partId, part.value]),
  );
  const pieces = body.htmlBody.flatMap((part): Piece[] => {
    const piece = pieceOf(part, values, context, options);
    return piece === null ? [] : [piece];
  });
  const adapted = options.adapt && options.theme === "dark" && !context.declaresDark;
  if (adapted) {
    for (const piece of pieces) {
      adaptPiece(piece);
    }
  }
  const padded = !pieces.some((piece) => paintsBackground(piece));
  const canvas = canvasOf(options.theme, adapted || context.declaresDark);
  const style = baseStyle(options.style, canvas, padded);
  const content = pieces.map((piece) => pieceHtml(piece)).join("");
  return {
    html: `<!doctype html><html><head>${HEAD}<style>${style}</style></head><body dir="auto">${content}</body></html>`,
    remote: images.remote,
    declaresDark: context.declaresDark,
    adapted,
    inlineParts: images.inlineParts,
  };
}
