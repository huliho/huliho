// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { QuotedLine } from "../text/flowed";

// The longest line the writer makes, quote marks and the soft break's
// space included (RFC 3676 section 4.2).
export const FLOWED_LINE_MAX = 78;

// The deepest quote written; a deeper line is written at this depth, so
// a quote of a quote cannot multiply the text it carries.
export const QUOTE_DEPTH_MAX = 32;

const QUOTE_MARK = ">";
// The signature separator, written as it stands (RFC 3676 section 4.3).
const SEPARATOR = "-- ";
// What a line may not open with unless a space stands in front of it
// (RFC 3676 section 4.4).
const STUFFED = /^(?: |>|From )/u;
// The longest opening STUFFED looks for.
const STUFFED_CHARS = "From ".length;
const LINE_BREAKS = /[\r\n]+/gu;
const TRAILING_SPACES = / +$/u;

function marksOf(depth: number): string {
  const level = Math.min(depth, QUOTE_DEPTH_MAX);
  return level === 0 ? "" : `${QUOTE_MARK.repeat(level)} `;
}

// Where a soft break cuts the characters: after the last space inside
// the room, so the space stays at the end of the line, else at the room's
// end inside a word, which DelSp=yes joins again.
function cutOf(characters: readonly string[], room: number): number {
  const last = characters.slice(0, room).lastIndexOf(" ");
  return last >= 0 ? last + 1 : room;
}

// One paragraph as the lines a flowed text carries it in: every line but
// the last ends in the soft break's space, which DelSp=yes deletes again.
function physicalLines({ depth, text }: QuotedLine): string[] {
  const marks = marksOf(depth);
  if (text === SEPARATOR) {
    return [`${marks}${SEPARATOR}`];
  }
  const kept = text.replaceAll(LINE_BREAKS, " ").replace(TRAILING_SPACES, "");
  if (kept === "") {
    return [marks.trimEnd()];
  }
  const lines: string[] = [];
  let rest = Array.from(kept);
  while (rest.length > 0) {
    // A quoted line's marks end in a space already, which unflow reads as the stuffing.
    const opening = rest.slice(0, STUFFED_CHARS).join("");
    const stuffing = marks === "" && STUFFED.test(opening) ? " " : "";
    const room = Math.max(1, FLOWED_LINE_MAX - marks.length - stuffing.length);
    if (rest.length <= room) {
      lines.push(`${marks}${stuffing}${rest.join("")}`);
      break;
    }
    const cut = cutOf(rest, Math.max(1, room - 1));
    lines.push(`${marks}${stuffing}${rest.slice(0, cut).join("")} `);
    rest = rest.slice(cut);
  }
  return lines;
}

// The lines as a text/plain part with format=flowed and delsp=yes (RFC
// 3676), each line ended by a line feed: a paragraph wraps at
// FLOWED_LINE_MAX, a quoted one carries its marks on every line, trailing
// spaces leave a fixed line and the separator stays as it is.
export function writeFlowed(lines: readonly QuotedLine[]): string {
  return lines.flatMap((line) => physicalLines(line).map((physical) => `${physical}\n`)).join("");
}

// The lines as a text/plain part without format=flowed: one line per
// paragraph, however long, with its quote marks in front.
export function writeFixed(lines: readonly QuotedLine[]): string {
  return lines
    .map(({ depth, text }) =>
      text === SEPARATOR
        ? `${marksOf(depth)}${SEPARATOR}`
        : `${marksOf(depth)}${text.replaceAll(LINE_BREAKS, " ")}`.replace(TRAILING_SPACES, ""),
    )
    .map((line) => `${line}\n`)
    .join("");
}
