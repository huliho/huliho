// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import * as fc from "fast-check";
import { expect, test } from "vitest";

import { quotedLines, unflow } from "../text/flowed";
import type { QuotedLine } from "../text/flowed";
import { FLOWED_LINE_MAX, QUOTE_DEPTH_MAX, writeFixed, writeFlowed } from "./flowed-write";

const SEPARATOR = "-- ";
const DEEPER = 40;
const LONG_WORD = 200;

// What a paragraph reads as once written: the separator as it stands,
// else line breaks as spaces with the trailing spaces gone.
function normalized({ depth, text }: QuotedLine): QuotedLine {
  const flat = text.replaceAll(/[\r\n]+/gu, " ").replace(/ +$/u, "");
  return { depth, text: text === SEPARATOR ? text : flat };
}

function physical(text: string): string[] {
  return text.split("\n").slice(0, -1);
}

const anyText = fc.oneof(
  fc.constantFrom("", SEPARATOR, "From here", ">quoted", " lead", "--", "tail   "),
  fc.string({
    unit: fc.constantFrom("a", " ", ">", "-", "F", "é", "語", "😀", "\n"),
    maxLength: 300,
  }),
  fc.string({ unit: "grapheme", maxLength: 200 }),
);

const anyLine = fc.record({ depth: fc.integer({ min: 0, max: QUOTE_DEPTH_MAX }), text: anyText });

test("every paragraph comes back from unflow as written, delsp=yes, rfc3676_4_1", () => {
  fc.assert(
    fc.property(fc.array(anyLine, { maxLength: 12 }), (lines) => {
      expect(unflow(writeFlowed(lines), true)).toEqual(lines.map(normalized));
    }),
  );
});

test("every line stays at or under 78 characters with its marks and soft space, rfc3676_4_2", () => {
  fc.assert(
    fc.property(fc.array(anyLine, { maxLength: 12 }), (lines) => {
      for (const line of physical(writeFlowed(lines))) {
        expect(Array.from(line).length).toBeLessThanOrEqual(FLOWED_LINE_MAX);
      }
    }),
  );
});

test("a line that opens with a space, a quote mark or From is stuffed, rfc3676_4_4", () => {
  expect(writeFlowed([{ depth: 0, text: "From the desk" }])).toBe(" From the desk\n");
  expect(writeFlowed([{ depth: 0, text: ">not a quote" }])).toBe(" >not a quote\n");
  expect(writeFlowed([{ depth: 0, text: " indented" }])).toBe("  indented\n");
  expect(writeFlowed([{ depth: 0, text: "Fromage" }])).toBe("Fromage\n");
});

test("a long paragraph breaks after a space and keeps that space before the soft one", () => {
  const words = Array.from({ length: 30 }, (_, index) => `word${String(index)}`).join(" ");
  const lines = physical(writeFlowed([{ depth: 0, text: words }]));
  expect(lines.length).toBeGreaterThan(1);
  for (const line of lines.slice(0, -1)) {
    expect(line.endsWith("  ")).toBe(true);
  }
  expect(lines.at(-1)?.endsWith(" ")).toBe(false);
});

test("a word longer than a line breaks inside, which delsp=yes joins again", () => {
  const word = "x".repeat(LONG_WORD);
  const text = writeFlowed([{ depth: 2, text: word }]);
  for (const line of physical(text)) {
    expect(line.startsWith(">> ")).toBe(true);
    expect(line.length).toBeLessThanOrEqual(FLOWED_LINE_MAX);
  }
  expect(unflow(text, true)).toEqual([{ depth: 2, text: word }]);
});

test("the separator stays as it is, its trailing space included, rfc3676_4_3", () => {
  const text = writeFlowed([
    { depth: 0, text: "Thanks," },
    { depth: 0, text: SEPARATOR },
    { depth: 0, text: "Sanne" },
  ]);
  expect(text).toBe("Thanks,\n-- \nSanne\n");
  expect(unflow(text, true).map((line) => line.text)).toEqual(["Thanks,", SEPARATOR, "Sanne"]);
});

test("an empty quoted line carries its marks and no space", () => {
  expect(writeFlowed([{ depth: 3, text: "" }])).toBe(">>>\n");
  expect(writeFlowed([{ depth: 0, text: "" }])).toBe("\n");
});

test("a quote deeper than the bound is written at the bound", () => {
  const text = writeFlowed([{ depth: DEEPER, text: "deep" }]);
  expect(text).toBe(`${">".repeat(QUOTE_DEPTH_MAX)} deep\n`);
});

test("the fixed form writes one line per paragraph with its marks, however long", () => {
  const long = "y".repeat(LONG_WORD);
  const text = writeFixed([
    { depth: 0, text: long },
    { depth: 1, text: "quoted  " },
    { depth: 2, text: "" },
    { depth: 0, text: SEPARATOR },
  ]);
  expect(text).toBe(`${long}\n> quoted\n>>\n-- \n`);
  expect(quotedLines(text)).toEqual([
    { depth: 0, text: long },
    { depth: 1, text: "quoted" },
    { depth: 2, text: "" },
    { depth: 0, text: SEPARATOR },
  ]);
});
