// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import * as fc from "fast-check";
import { expect, test } from "vitest";

import { flowedOf, quotedLines, unflow } from "./flowed";
import type { QuotedLine } from "./flowed";

// The longest header value the module reads.
const HEADER_MAX_CHARS = 1024;
// The deepest quote the round trip writes.
const DEPTH_MAX = 5;
const SEPARATOR = "-- ";
// The line starts a sender stuffs (RFC 3676 section 4.4).
const STUFFED_STARTS = [" ", ">", "From "];

function flat(text: string): QuotedLine {
  return { depth: 0, text };
}

function quoted(depth: number, text: string): QuotedLine {
  return { depth, text };
}

test("a flowed text/plain header answers its DelSp parameter (RFC 3676 section 4)", () => {
  expect(flowedOf(" text/plain; charset=utf-8; format=flowed; delsp=yes")).toEqual({ delSp: true });
  expect(flowedOf("text/plain; format=flowed")).toEqual({ delSp: false });
  expect(flowedOf("text/plain; format=flowed; delsp=no")).toEqual({ delSp: false });
  expect(flowedOf("text/plain; format=flowed; delsp=maybe")).toEqual({ delSp: false });
});

test("the type, the parameter names and their values match in any case (RFC 3676 section 4)", () => {
  expect(flowedOf("TEXT/Plain; Format=Flowed; DelSp=Yes")).toEqual({ delSp: true });
});

test("a quoted parameter value reads like a bare one", () => {
  expect(flowedOf('text/plain; format="flowed"; delsp="yes"')).toEqual({ delSp: true });
  expect(flowedOf('text/plain; format="Flowed"; delsp="no"')).toEqual({ delSp: false });
});

test("a header folded over lines reads like one on a single line", () => {
  const folded = " text/plain;\r\n charset=utf-8;\r\n\tformat=flowed;\r\n delsp = yes";
  expect(flowedOf(folded)).toEqual({ delSp: true });
  // A server that leaves the closing line break on the value changes nothing.
  expect(flowedOf(" text/plain; format=flowed\r\n")).toEqual({ delSp: false });
});

test("a semicolon inside a quoted value starts no parameter", () => {
  expect(flowedOf('text/plain; name="a; format=flowed"')).toBeNull();
  expect(flowedOf('text/plain; name="a\\"; format=flowed"')).toBeNull();
  expect(flowedOf('text/plain; name="a; b"; format=flowed')).toEqual({ delSp: false });
});

test("the first of a repeated parameter stands", () => {
  expect(flowedOf("text/plain; format=fixed; format=flowed")).toBeNull();
  expect(flowedOf("text/plain; format=flowed; format=fixed")).toEqual({ delSp: false });
});

test.each([
  null,
  "",
  "text/plain",
  "text/plain; charset=utf-8",
  "text/plain; format=fixed",
  "text/plain; format=other",
  "text/plain; delsp=yes",
  "text/html; format=flowed",
  "text/plain-ish; format=flowed",
  "format=flowed",
  "text/plain; format",
  "text/plain; =flowed",
])("%j answers null: the text is fixed", (header) => {
  expect(flowedOf(header)).toBeNull();
});

test("a header value past 1024 characters answers null", () => {
  const header = "text/plain; format=flowed;";
  expect(flowedOf(header.padEnd(HEADER_MAX_CHARS))).toEqual({ delSp: false });
  expect(flowedOf(header.padEnd(HEADER_MAX_CHARS + 1))).toBeNull();
});

test("any header value at all answers without a throw, null when it never says flowed", () => {
  const anyHeader = fc.oneof(
    fc.string({ unit: "binary" }),
    fc.string({
      unit: fc.constantFrom(";", "=", '"', "\\", " ", "\r\n", "text/plain", "format", "flowed"),
    }),
  );
  fc.assert(
    fc.property(anyHeader, (header) => {
      expect(flowedOf(header) === null || header.includes("flowed")).toBe(true);
    }),
  );
});

test("soft breaks join into paragraphs between empty fixed lines (RFC 3676 section 4.7)", () => {
  const text = [
    "`Take some more tea,' the March Hare said to Alice, very ",
    "earnestly.",
    "",
    "`I've had nothing yet,' Alice replied in an offended tone, `so ",
    "I can't take more.'",
    "",
    "`You mean you can't take LESS,' said the Hatter: `it's very ",
    "easy to take MORE than nothing.'",
    "",
  ].join("\r\n");
  expect(unflow(text, false)).toEqual([
    flat("`Take some more tea,' the March Hare said to Alice, very earnestly."),
    flat(""),
    flat("`I've had nothing yet,' Alice replied in an offended tone, `so I can't take more.'"),
    flat(""),
    flat(
      "`You mean you can't take LESS,' said the Hatter: `it's very easy to take MORE than nothing.'",
    ),
  ]);
});

test("quoted lines keep their depth and a quoted paragraph joins (RFC 3676 section 4.7)", () => {
  const text = [
    ">>>Take some more tea.",
    ">>I've had nothing yet, so I can't take more.",
    ">You mean you can't take LESS, it's very easy to take ",
    ">MORE than nothing.",
    "",
  ].join("\r\n");
  expect(unflow(text, false)).toEqual([
    quoted(3, "Take some more tea."),
    quoted(2, "I've had nothing yet, so I can't take more."),
    quoted(1, "You mean you can't take LESS, it's very easy to take MORE than nothing."),
  ]);
});

test("a flowed line before a line of another depth ends its paragraph (RFC 3676 section 4.5)", () => {
  const text = [
    "> Thou villainous ill-breeding spongy dizzy-eyed ",
    "> reeky elf-skinned pigeon-egg! ",
    ">> Thou artless swag-bellied milk-livered ",
    ">> dismal-dreaming idle-headed scut!",
    "",
  ].join("\r\n");
  expect(unflow(text, false)).toEqual([
    quoted(1, "Thou villainous ill-breeding spongy dizzy-eyed reeky elf-skinned pigeon-egg! "),
    quoted(2, "Thou artless swag-bellied milk-livered dismal-dreaming idle-headed scut!"),
  ]);
  expect(unflow("> one \n>> two \nthree\n", true)).toEqual([
    quoted(1, "one"),
    quoted(2, "two"),
    flat("three"),
  ]);
});

test("a line ending in a space joins the next line of the same depth (RFC 3676 section 4.1)", () => {
  expect(unflow("one \ntwo \nthree\n", false)).toEqual([flat("one two three")]);
  expect(unflow("> one \n> two\n", false)).toEqual([quoted(1, "one two")]);
  expect(unflow("one\ntwo\n", false)).toEqual([flat("one"), flat("two")]);
});

test("DelSp deletes the one space before a soft break (RFC 3676 section 4.1)", () => {
  expect(unflow("日本 \n語\n", true)).toEqual([flat("日本語")]);
  expect(unflow("one  \ntwo\n", true)).toEqual([flat("one two")]);
  expect(unflow("one  \ntwo\n", false)).toEqual([flat("one  two")]);
});

test("one stuffing space comes off the start of a line (RFC 3676 section 4.4)", () => {
  expect(unflow(" From here\n >not quoted\n  indented\n", false)).toEqual([
    flat("From here"),
    flat(">not quoted"),
    flat(" indented"),
  ]);
});

test("the quote marks are counted before the stuffing space comes off (RFC 3676 section 4.5)", () => {
  const text = ">> Exit, Stage Left\n>>Exit, Stage Left\n> > Exit, Stage Left\n";
  expect(unflow(text, false)).toEqual([
    quoted(2, "Exit, Stage Left"),
    quoted(2, "Exit, Stage Left"),
    quoted(1, "> Exit, Stage Left"),
  ]);
});

test("the signature separator joins nothing and ends the paragraph before it (RFC 3676 section 4.3)", () => {
  expect(unflow("Bye \n-- \nSanne\n", false)).toEqual([flat("Bye "), flat("-- "), flat("Sanne")]);
  expect(unflow("Bye \n-- \nSanne\n", true)).toEqual([flat("Bye"), flat("-- "), flat("Sanne")]);
});

test("a quoted or stuffed separator is still the separator (RFC 3676 section 4.3)", () => {
  expect(unflow(">-- \n>Sanne\n> -- \n>Sanne\n -- \nSanne\n", true)).toEqual([
    quoted(1, "-- "),
    quoted(1, "Sanne"),
    quoted(1, "-- "),
    quoted(1, "Sanne"),
    flat("-- "),
    flat("Sanne"),
  ]);
});

test("two dashes with another count of spaces are an ordinary line (RFC 3676 section 4.3)", () => {
  expect(unflow("--  \nrest\n", true)).toEqual([flat("-- rest")]);
  expect(unflow("--  \nrest\n", false)).toEqual([flat("--  rest")]);
  expect(unflow("--\nrest\n", false)).toEqual([flat("--"), flat("rest")]);
});

test("lines end in CRLF or LF and the last line needs neither", () => {
  const paragraphs = [flat("one two"), quoted(1, "three"), flat("")];
  expect(unflow("one \r\ntwo\r\n>three\r\n\r\n", false)).toEqual(paragraphs);
  expect(unflow("one \ntwo\n>three\n\n", false)).toEqual(paragraphs);
  expect(unflow("one \r\ntwo\n>three\r\n\n", false)).toEqual(paragraphs);
  expect(unflow("one \r\ntwo", false)).toEqual([flat("one two")]);
});

test("an empty line is a fixed line, quoted or not (RFC 3676 section 4.1)", () => {
  expect(unflow("", false)).toEqual([]);
  expect(unflow("\n", false)).toEqual([flat("")]);
  expect(unflow("one\n\ntwo\n", false)).toEqual([flat("one"), flat(""), flat("two")]);
  expect(unflow(">\n> \n", false)).toEqual([quoted(1, ""), quoted(1, "")]);
  expect(unflow("one \n\ntwo\n", false)).toEqual([flat("one "), flat("two")]);
});

test("a line of spaces alone is a flowed line (RFC 3676 section 4.1)", () => {
  expect(unflow("  \none\n", false)).toEqual([flat(" one")]);
  expect(unflow("  \none\n", true)).toEqual([flat("one")]);
});

test("a flowed last line ends its paragraph at the end of the text (RFC 3676 section 4.1)", () => {
  expect(unflow("one \ntwo ", false)).toEqual([flat("one two ")]);
  expect(unflow("one \ntwo \n", true)).toEqual([flat("onetwo")]);
});

const lineEnd = fc.constantFrom("\r\n", "\n");

const fixedLine = fc
  .string({ unit: fc.constantFrom(">", " ", "-", "a", "é", "語") })
  .filter((line) => !line.endsWith(" "));

test("fixed lines alone answer one paragraph each at the depth of their quote marks (RFC 3676 section 4.5)", () => {
  fc.assert(
    fc.property(fc.array(fixedLine), lineEnd, fc.boolean(), (lines, end, delSp) => {
      const text = lines.map((line) => line + end).join("");
      expect(unflow(text, delSp)).toEqual(
        lines.map((line) => quoted(line.search(/[^>]|$/u), line.replace(/^>* ?/u, ""))),
      );
    }),
  );
});

interface Word {
  text: string;
  // Whether the sender breaks the line after this word.
  soft: boolean;
}

interface Paragraph {
  depth: number;
  words: Word[];
}

const word = fc.record({
  text: fc.oneof(
    fc.constantFrom("", "From", ">", "--"),
    fc.string({ unit: fc.constantFrom("a", "é", "語", "-", ">") }),
  ),
  soft: fc.boolean(),
});

// The last line of a paragraph is fixed, so its text never ends in a space.
const paragraph = fc
  .record({ depth: fc.integer({ min: 0, max: DEPTH_MAX }), words: fc.array(word) })
  .filter(({ words }) => words.at(-1)?.text !== "");

function stuffed(line: string): string {
  return STUFFED_STARTS.some((start) => line.startsWith(start)) ? ` ${line}` : line;
}

// The lines a sender writes (RFC 3676 section 4.2): a soft break follows the
// space between two words, DelSp adds a space of its own and no break leaves
// the separator standing as a line.
function encoded({ depth, words }: Paragraph, delSp: boolean): string[] {
  const flow = delSp ? "  " : " ";
  const lines: string[] = [];
  let line = "";
  for (const { text, soft } of words.slice(0, -1)) {
    const flowed = line + text + flow;
    if (soft && flowed !== SEPARATOR) {
      lines.push(flowed);
      line = "";
    } else {
      line += `${text} `;
    }
  }
  lines.push(line + (words.at(-1)?.text ?? ""));
  return lines.map((content) => ">".repeat(depth) + stuffed(content));
}

test.each([false, true])(
  "flowed text answers the paragraphs it was written from, DelSp %j (RFC 3676 section 4.2)",
  (delSp) => {
    fc.assert(
      fc.property(fc.array(paragraph), lineEnd, (paragraphs, end) => {
        const lines = paragraphs.flatMap((one) => encoded(one, delSp));
        const text = lines.map((line) => line + end).join("");
        expect(unflow(text, delSp)).toEqual(
          paragraphs.map(({ depth, words }) =>
            quoted(depth, words.map((one) => one.text).join(" ")),
          ),
        );
      }),
    );
  },
);

test("any text at all answers without a throw and never more text than it holds", () => {
  const anyText = fc.oneof(
    fc.string({ unit: "binary" }),
    fc.string({ unit: fc.constantFrom(">", " ", "-", "\r", "\n", "a") }),
  );
  fc.assert(
    fc.property(anyText, fc.boolean(), (text, delSp) => {
      const answered = unflow(text, delSp).reduce((sum, one) => sum + one.text.length, 0);
      expect(answered).toBeLessThanOrEqual(text.length);
    }),
  );
});

test("fixed text keeps every line, its quote marks counted and one space after them taken off", () => {
  expect(quotedLines("Hi,\n\n> one\n>> two\n>>>three\n> \nend ")).toEqual([
    flat("Hi,"),
    flat(""),
    quoted(1, "one"),
    quoted(2, "two"),
    quoted(3, "three"),
    quoted(1, ""),
    flat("end "),
  ]);
  expect(quotedLines("a\r\nb\r\n")).toEqual([flat("a"), flat("b")]);
  expect(quotedLines("")).toEqual([]);
});

test("an unquoted line of fixed text keeps the spaces it starts with, as a patch in a mail needs", () => {
  expect(quotedLines(" context\n+added\n  two deep\n>  quoted")).toEqual([
    flat(" context"),
    flat("+added"),
    flat("  two deep"),
    quoted(1, " quoted"),
  ]);
});

test("fixed text of any lines keeps an unquoted line whole and reads a quoted one as flowed text does", () => {
  const anyLine = fc.string({ unit: fc.constantFrom(">", " ", "-", "a", "é", "語") });
  fc.assert(
    fc.property(fc.array(anyLine), lineEnd, (lines, end) => {
      const text = lines.map((line) => line + end).join("");
      expect(quotedLines(text)).toEqual(
        lines.map((line) =>
          line.startsWith(">")
            ? quoted(line.search(/[^>]|$/u), line.replace(/^>* ?/u, ""))
            : flat(line),
        ),
      );
    }),
  );
});
