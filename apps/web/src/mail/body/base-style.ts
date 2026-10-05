// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The space between a mail that paints no background and the card's
// edge, in CSS pixels.
const FRAME_PADDING = 16;

// The page colors of a mail shown as it was sent, which the dark theme
// would otherwise show through.
const AS_SENT_CANVAS = "#ffffff";
const AS_SENT_TEXT = "#000000";

// The type size of the alt text in a blocked image's box, in CSS pixels.
const BLOCKED_TYPE = 12;

// The outline of a blocked image's box, in CSS pixels; the image's edge
// cuts half of it off.
const BLOCKED_OUTLINE = 2;

// The longest alt text a blocked image's box shows.
const ALT_CHARS_MAX = 80;

// What the frame takes from the card around it, as computed values,
// since custom properties do not cross into another document.
export interface FrameStyle {
  fontFamily: string;
  fontSize: string;
  lineHeight: string;
  // The page behind the cards, which a blocked image's box is filled with.
  bg: string;
  // The card's own surface.
  surface: string;
  border: string;
  text: string;
  muted: string;
}

// The canvas a mail stands on. Light: the card's surface shows through.
// Dark: the card's dark surface, for a mail that reads on it. As sent:
// the white page of a mail the dark theme shows unchanged.
export type Canvas = "light" | "dark" | "as-sent";

// A computed value as one declaration value; anything that could end
// the declaration or the element it stands in leaves.
function value(computed: string): string {
  return computed.replace(/[;{}<>\\]/g, "");
}

function canvasRule(style: FrameStyle, canvas: Canvas): string {
  switch (canvas) {
    case "dark":
      return `color-scheme: dark; background: ${value(style.surface)}; color: ${value(style.text)};`;
    case "as-sent":
      return `color-scheme: light; background: ${AS_SENT_CANVAS}; color: ${AS_SENT_TEXT};`;
    default:
      return `color-scheme: light; color: ${value(style.text)};`;
  }
}

// The style the frame's document starts with: the root as tall as its
// content, the card's type, the canvas and room around a mail that
// paints no background of its own.
export function baseStyle(style: FrameStyle, canvas: Canvas, padded: boolean): string {
  const padding = padded ? ` padding: ${String(FRAME_PADDING)}px;` : "";
  return [
    "html, body { margin: 0; height: auto !important; }",
    `html { font-family: ${value(style.fontFamily)}; font-size: ${value(style.fontSize)}; ${canvasRule(style, canvas)} }`,
    `body { line-height: ${value(style.lineHeight)}; overflow-x: auto; overflow-y: hidden; overflow-wrap: break-word;${padding} }`,
    "img { max-width: 100%; }",
  ].join("\n");
}

const XML_ESCAPES = new Map([
  ["&", "&amp;"],
  ["<", "&lt;"],
  [">", "&gt;"],
  ['"', "&quot;"],
  ["'", "&apos;"],
]);

// The code points XML admits: tab, line feed, carriage return and
// everything from the space on but two noncharacters (XML 1.0 section
// 2.2).
const SPACE = 0x20;
const XML_WHITESPACE = new Set([0x09, 0x0a, 0x0d]);
const XML_NONCHARACTERS = new Set([0xfffe, 0xffff]);

function isXmlChar(character: string): boolean {
  const code = character.codePointAt(0) ?? 0;
  return XML_WHITESPACE.has(code) || (code >= SPACE && !XML_NONCHARACTERS.has(code));
}

// Text as XML character data: the five markup characters escaped and
// every character XML does not admit removed.
function xmlText(text: string): string {
  return Array.from(text.toWellFormed())
    .filter((character) => isXmlChar(character))
    .join("")
    .replace(/[&<>"']/g, (character) => XML_ESCAPES.get(character) ?? "");
}

// The image that stands where a remote image is blocked: a box in the
// place the sender gave it, filled and outlined, with the alt text in
// the middle, set in the card's type. It is an image itself, so the
// element and every rule that sizes it stay as the sender wrote them.
export function blockedImage(alt: string, style: FrameStyle): string {
  const shown = alt.length > ALT_CHARS_MAX ? `${alt.slice(0, ALT_CHARS_MAX)}…` : alt;
  const svg = [
    '<svg xmlns="http://www.w3.org/2000/svg" width="100%" height="100%">',
    `<rect width="100%" height="100%" fill="${xmlText(style.bg)}" stroke="${xmlText(style.border)}" stroke-width="${String(BLOCKED_OUTLINE)}"/>`,
    `<text x="50%" y="50%" text-anchor="middle" dominant-baseline="central" font-family="${xmlText(style.fontFamily)}" font-size="${String(BLOCKED_TYPE)}" fill="${xmlText(style.muted)}">${xmlText(shown)}</text>`,
    "</svg>",
  ].join("");
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}
