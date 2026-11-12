// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { z } from "../schema";
import type { QuotedLine } from "../text/flowed";

// One mark on a run of text, as ProseMirror writes it to JSON.
export interface DocumentMark {
  type: string;
  attrs?: Record<string, unknown>;
}

// One node of the composer's document, as ProseMirror writes it to JSON.
export interface DocumentNode {
  type: string;
  attrs?: Record<string, unknown>;
  content?: DocumentNode[];
  marks?: DocumentMark[];
  text?: string;
}

const attrsSchema = z.exactOptional(z.record(z.string(), z.unknown()));

const markSchema: z.ZodMiniType<DocumentMark> = z.object({
  type: z.string(),
  attrs: attrsSchema,
});

export const documentSchema: z.ZodMiniType<DocumentNode> = z.object({
  type: z.string(),
  attrs: attrsSchema,
  get content() {
    return z.exactOptional(z.array(documentSchema));
  },
  marks: z.exactOptional(z.array(markSchema)),
  text: z.exactOptional(z.string()),
});

// What a list item's first line opens with, by the kind of list.
const BULLET = "- ";
// What one level of a list item's lines is indented by below its marker.
const LIST_INDENT = "  ";
// What a horizontal rule writes as.
const RULE = "----";

// Where a block's lines stand: how deep they are quoted and what the
// first line and the lines after it open with.
interface Place {
  depth: number;
  first: string;
  rest: string;
}

function attribute(node: DocumentNode | DocumentMark, name: string): unknown {
  return new Map(Object.entries(node.attrs ?? {})).get(name);
}

// A link writes its text with the address in angle brackets where the
// two differ, so a plain reader still sees where it leads.
function runOf(node: DocumentNode): string {
  const text = node.text ?? "";
  const link = node.marks?.find((mark) => mark.type === "link");
  const href = link === undefined ? undefined : attribute(link, "href");
  return typeof href === "string" && href !== text ? `${text} <${href}>` : text;
}

// The lines of one textblock: a hard break opens the next line.
function textLines(node: DocumentNode): string[] {
  const lines: string[] = [];
  let line = "";
  for (const child of node.content ?? []) {
    if (child.type === "hardBreak") {
      lines.push(line);
      line = "";
    } else if (child.type === "text") {
      line += runOf(child);
    }
  }
  return [...lines, line];
}

function placed(place: Place, texts: readonly string[]): QuotedLine[] {
  return texts.map((text, index) => ({
    depth: place.depth,
    text: `${index === 0 ? place.first : place.rest}${text}`,
  }));
}

// The lines of the blocks inside a container, the first of them taking
// the container's opening and the rest its indent.
function blockLines(nodes: readonly DocumentNode[], place: Place): QuotedLine[] {
  return nodes.flatMap((node, index) =>
    linesOf(node, index === 0 ? place : { ...place, first: place.rest }),
  );
}

function listLines(node: DocumentNode, place: Place): QuotedLine[] {
  const start = attribute(node, "start");
  const first = typeof start === "number" && Number.isInteger(start) ? start : 1;
  const ordered = node.type === "orderedList";
  return (node.content ?? []).flatMap((item, index) => {
    const marker = ordered ? `${String(first + index)}. ` : BULLET;
    const indent = `${place.rest}${LIST_INDENT}`;
    const items = { depth: place.depth, first: `${place.rest}${marker}`, rest: indent };
    return blockLines(
      item.content ?? [],
      index === 0 ? { ...items, first: `${place.first}${marker}` } : items,
    );
  });
}

function linesOf(node: DocumentNode, place: Place): QuotedLine[] {
  switch (node.type) {
    case "paragraph":
      return placed(place, textLines(node));
    case "blockquote":
      return blockLines(node.content ?? [], { depth: place.depth + 1, first: "", rest: "" });
    case "bulletList":
    case "orderedList":
      return listLines(node, place);
    case "horizontalRule":
      return placed(place, [RULE]);
    case "image":
      return [];
    default:
      return blockLines(node.content ?? [], place);
  }
}

// The document as the lines of a plain text, each with its quote depth:
// a paragraph per line, a link as its text with the address in angle
// brackets, a list item behind a hyphen or its number and a quote one
// depth deeper. A picture writes nothing; a node of another kind writes
// what it holds.
export function documentLines(document: DocumentNode): QuotedLine[] {
  return linesOf(document, { depth: 0, first: "", rest: "" });
}
