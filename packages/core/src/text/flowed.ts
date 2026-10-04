// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { parse } from "content-type";

// A paragraph of plain text and how deep it is quoted.
export interface QuotedLine {
  depth: number;
  text: string;
}

// Bounds the scan of a hostile header; a real Content-Type stays far below it.
const CONTENT_TYPE_MAX_CHARS = 1024;
// A line break ahead of a space or tab folds a header (RFC 5322 section 2.2.3).
const FOLD = /\r?\n(?=[ \t])/g;
const QUOTE_MARK = ">";
// The signature separator, neither flowed nor fixed (RFC 3676 section 4.3).
const SEPARATOR = "-- ";

// The flowed parameters of a Content-Type header value, null for fixed
// text. Names and values compare without case (RFC 3676 section 4).
export function flowedOf(contentType: string | null): { delSp: boolean } | null {
  if (contentType === null || contentType.length > CONTENT_TYPE_MAX_CHARS) {
    return null;
  }
  const { type, parameters } = parse(contentType.replaceAll(FOLD, "").trim());
  const read = new Map(Object.entries(parameters));
  if (type !== "text/plain" || read.get("format")?.toLowerCase() !== "flowed") {
    return null;
  }
  return { delSp: read.get("delsp")?.toLowerCase() === "yes" };
}

interface Line {
  depth: number;
  text: string;
  flowed: boolean;
  separator: boolean;
}

// A text that ends in a line break has no line after it.
function linesOf(text: string): string[] {
  const lines = text.split("\n");
  if (lines.at(-1) === "") {
    lines.pop();
  }
  return lines.map((line) => (line.endsWith("\r") ? line.slice(0, -1) : line));
}

function quoteDepth(line: string): number {
  let depth = 0;
  while (line.charAt(depth) === QUOTE_MARK) {
    depth += 1;
  }
  return depth;
}

// Quote marks come off first, then one stuffing space, then the flow space
// that DelSp deletes (RFC 3676 section 4.1).
function lineOf(raw: string, delSp: boolean): Line {
  const depth = quoteDepth(raw);
  const content = raw.slice(raw.charAt(depth) === " " ? depth + 1 : depth);
  const separator = content === SEPARATOR;
  const flowed = content.endsWith(" ") && !separator;
  return { depth, text: flowed && delSp ? content.slice(0, -1) : content, flowed, separator };
}

// RFC 3676 section 4.1: the paragraphs of a flowed text.
export function unflow(text: string, delSp: boolean): QuotedLine[] {
  const paragraphs: { depth: number; parts: string[] }[] = [];
  let open: { depth: number; parts: string[] } | null = null;
  for (const raw of linesOf(text)) {
    const line = lineOf(raw, delSp);
    // A flowed line ends its paragraph before another depth or a separator.
    if (open === null || open.depth !== line.depth || line.separator) {
      open = { depth: line.depth, parts: [] };
      paragraphs.push(open);
    }
    open.parts.push(line.text);
    if (!line.flowed) {
      open = null;
    }
  }
  return paragraphs.map(({ depth, parts }) => ({ depth, text: parts.join("") }));
}
