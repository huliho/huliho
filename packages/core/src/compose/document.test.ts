// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { documentLines, documentSchema } from "./document";
import type { DocumentNode } from "./document";

function text(value: string, href?: string): DocumentNode {
  return href === undefined
    ? { type: "text", text: value }
    : { type: "text", text: value, marks: [{ type: "link", attrs: { href } }] };
}

function paragraph(...content: DocumentNode[]): DocumentNode {
  return { type: "paragraph", content };
}

function item(...content: DocumentNode[]): DocumentNode {
  return { type: "listItem", content };
}

function doc(...content: DocumentNode[]): DocumentNode {
  return { type: "doc", content };
}

test("a paragraph is a line and a hard break opens the next", () => {
  const lines = documentLines(
    doc(
      paragraph(text("Dear "), { type: "text", text: "Mo", marks: [{ type: "bold" }] }),
      paragraph(),
      paragraph(text("one"), { type: "hardBreak" }, text("two")),
    ),
  );
  expect(lines).toEqual([
    { depth: 0, text: "Dear Mo" },
    { depth: 0, text: "" },
    { depth: 0, text: "one" },
    { depth: 0, text: "two" },
  ]);
});

test("a link writes its text with the address in angle brackets where they differ", () => {
  const lines = documentLines(
    doc(
      paragraph(
        text("the report", "https://example.test/r"),
        text(" and "),
        text("https://example.test", "https://example.test"),
      ),
    ),
  );
  expect(lines).toEqual([
    { depth: 0, text: "the report <https://example.test/r> and https://example.test" },
  ]);
});

test("a list item writes behind a hyphen or its number, nested items indented", () => {
  const lines = documentLines(
    doc(
      {
        type: "bulletList",
        content: [
          item(paragraph(text("milk"))),
          item(paragraph(text("bread")), {
            type: "bulletList",
            content: [item(paragraph(text("rye")))],
          }),
        ],
      },
      {
        type: "orderedList",
        attrs: { start: 3 },
        content: [
          item(paragraph(text("third")), paragraph(text("more"))),
          item(paragraph(text("fourth"))),
        ],
      },
    ),
  );
  expect(lines.map((line) => line.text)).toEqual([
    "- milk",
    "- bread",
    "  - rye",
    "3. third",
    "  more",
    "4. fourth",
  ]);
});

test("a quote writes one depth deeper, a quote in a quote two", () => {
  const lines = documentLines(
    doc(paragraph(text("Sure.")), {
      type: "blockquote",
      content: [
        paragraph(text("Lunch?")),
        { type: "blockquote", content: [paragraph(text("Hungry"))] },
      ],
    }),
  );
  expect(lines).toEqual([
    { depth: 0, text: "Sure." },
    { depth: 1, text: "Lunch?" },
    { depth: 2, text: "Hungry" },
  ]);
});

test("a rule writes a line of hyphens, a picture nothing and another node what it holds", () => {
  const lines = documentLines(
    doc(
      { type: "horizontalRule" },
      { type: "image", attrs: { src: "cid:logo" } },
      { type: "quoteFold", content: [paragraph(text("inside"))] },
    ),
  );
  expect(lines).toEqual([
    { depth: 0, text: "----" },
    { depth: 0, text: "inside" },
  ]);
});

test("a stored document passes its schema and a broken one fails it", () => {
  const stored = doc(paragraph(text("hi", "https://example.test")));
  expect(documentSchema.parse(stored)).toEqual(stored);
  expect(documentSchema.safeParse({ type: "doc", content: [{ content: [] }] }).success).toBe(false);
  expect(documentSchema.safeParse({ type: "doc", content: "text" }).success).toBe(false);
});
