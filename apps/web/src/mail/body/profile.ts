// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import createPurifier from "dompurify";
import type { Config, UponSanitizeAttributeHookEvent } from "dompurify";

// The elements a mail keeps: structure, text, lists, tables, inline
// marks, images and its style blocks. The server's sanitizer holds the
// same list; a test keeps the two equal.
export const TAGS = [
  "a",
  "abbr",
  "article",
  "b",
  "bdi",
  "bdo",
  "big",
  "blockquote",
  "br",
  "caption",
  "center",
  "cite",
  "code",
  "col",
  "colgroup",
  "dd",
  "del",
  "details",
  "dfn",
  "div",
  "dl",
  "dt",
  "em",
  "figcaption",
  "figure",
  "font",
  "footer",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "header",
  "hr",
  "i",
  "img",
  "ins",
  "kbd",
  "li",
  "main",
  "mark",
  "nav",
  "ol",
  "p",
  "pre",
  "q",
  "s",
  "samp",
  "section",
  "small",
  "span",
  "strike",
  "strong",
  "style",
  "sub",
  "summary",
  "sup",
  "table",
  "tbody",
  "td",
  "tfoot",
  "th",
  "thead",
  "time",
  "tr",
  "tt",
  "u",
  "ul",
  "var",
  "wbr",
] as const;

// The attributes any kept element may carry.
export const GENERIC_ATTRIBUTES = [
  "align",
  "alt",
  "bgcolor",
  "border",
  "cellpadding",
  "cellspacing",
  "class",
  "color",
  "colspan",
  "dir",
  "face",
  "height",
  "id",
  "lang",
  "rowspan",
  "size",
  "style",
  "title",
  "valign",
  "width",
] as const;

// The URL attributes, each on the elements that carry it.
export const URL_ATTRIBUTES: readonly (readonly [string, string])[] = [
  ["a", "href"],
  ["img", "src"],
  ["table", "background"],
  ["tr", "background"],
  ["td", "background"],
];

// Every link opens in a new tab that knows nothing of this one.
export const LINK_TARGET = "_blank";
export const LINK_REL = "noopener noreferrer";

// The schemes a URL attribute may carry at all; the URL policy narrows
// them per attribute. A value that names no scheme passes here, since
// the same test meets `width` and `align`; the policy drops a relative
// URL.
const ALLOWED_URI = /^(?:(?:https?|mailto|cid):|data:image\/|[^a-z]|[a-z+.-]+(?:[^a-z+.:-]|$))/i;

const CONFIG = {
  ALLOWED_TAGS: [...TAGS],
  ALLOWED_ATTR: [...GENERIC_ATTRIBUTES, ...new Set(URL_ATTRIBUTES.map(([, name]) => name))],
  ALLOWED_URI_REGEXP: ALLOWED_URI,
  ALLOW_DATA_ATTR: false,
  ALLOW_ARIA_ATTR: false,
  FORCE_BODY: true,
  RETURN_DOM: true as const,
} satisfies Config;

function carriesUrl(element: string, attribute: string): boolean {
  return URL_ATTRIBUTES.some(([tag, name]) => tag === element && name === attribute);
}

function isUrlAttribute(attribute: string): boolean {
  return URL_ATTRIBUTES.some(([, name]) => name === attribute);
}

// A data URL that is no image. The sanitizer admits any data URL as an
// image's source beside the pattern above; the server's layer does not.
const OTHER_DATA = /^\s*data:(?!image\/)/i;

// A URL attribute stays on the elements that carry it and nowhere else,
// and holds no data URL but an image.
function keepUrlAttributes(node: Element, event: UponSanitizeAttributeHookEvent): void {
  if (!isUrlAttribute(event.attrName)) {
    return;
  }
  if (!carriesUrl(node.localName, event.attrName) || OTHER_DATA.test(event.attrValue)) {
    event.keepAttr = false;
  }
}

// The sender's target and rel are gone by now; every link gets ours.
function aimLinks(node: Element): void {
  if (node.localName === "a") {
    node.setAttribute("target", LINK_TARGET);
    node.setAttribute("rel", LINK_REL);
  }
}

// One sanitizer of its own, so its hooks meet no other caller's HTML.
const purifier = createPurifier(window);
purifier.addHook("uponSanitizeAttribute", keepUrlAttributes);
purifier.addHook("afterSanitizeAttributes", aimLinks);

// The window's layer of the mail sanitizer: the HTML parsed into a
// document that runs and loads nothing, with everything off the
// allowlist removed. The answer is the body of that document; a value
// the sanitizer makes no body of renders as nothing.
export function purify(html: string): HTMLElement {
  const body = purifier.sanitize(html, CONFIG);
  return body instanceof HTMLElement ? body : document.implementation.createHTMLDocument("").body;
}
