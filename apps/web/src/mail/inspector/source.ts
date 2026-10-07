// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The first part of a message the source view reads; the rest is the download's.
export const SOURCE_VIEW_BYTES = 512 * 1024;

const OK = 200;
// The answer to a range the route honors (RFC 9110 section 15.3.7).
const PARTIAL_CONTENT = 206;
// What a partial answer says of itself: the bytes sent over the whole
// (RFC 9110 section 14.4).
const CONTENT_RANGE = /^bytes (\d+)-(\d+)\/(\d+|\*)$/;
// The bidi controls a sender can turn text around with: the marks, the
// embeddings and overrides and the isolates (Unicode chapter 23.2).
const BIDI_CONTROLS = /\p{Bidi_Control}/gu;
const HEX = 16;

export interface Source {
  text: string;
  // Whether the message runs past what was read.
  more: boolean;
}

// An answer other than the bytes asked for.
export class SourceError extends Error {
  readonly status: number;

  constructor(status: number) {
    super(`the source answered ${String(status)}`);
    this.name = "SourceError";
    this.status = status;
  }
}

// Ends the read at the cap; the stream behind it is let go.
class PastCap extends Error {}

// Whether a partial answer stops short of the whole.
function moreAfter(contentRange: string | null): boolean {
  const match = contentRange === null ? null : CONTENT_RANGE.exec(contentRange);
  if (match === null) {
    return true;
  }
  const [, , end = "", total = "*"] = match;
  return total === "*" || Number(end) + 1 < Number(total);
}

// Reads the stream up to the cap as UTF-8 (a replacement character
// stands for bytes outside it) and lets the stream go the moment it
// runs past.
async function readUpTo(body: ReadableStream<Uint8Array>, cap: number): Promise<Source> {
  const decoder = new TextDecoder();
  let text = "";
  let read = 0;
  const sink = new WritableStream<Uint8Array>({
    write(value) {
      if (read + value.length > cap) {
        text += decoder.decode(value.subarray(0, cap - read), { stream: true });
        throw new PastCap();
      }
      text += decoder.decode(value, { stream: true });
      read += value.length;
    },
  });
  try {
    await body.pipeTo(sink);
  } catch (reason: unknown) {
    if (!(reason instanceof PastCap)) {
      throw reason;
    }
    return { text: text + decoder.decode(), more: true };
  }
  return { text: text + decoder.decode(), more: false };
}

// The first `cap` bytes of the blob at `url` as text. The route answers
// a range from byte zero where it knows the blob's length and the whole
// blob otherwise, so the read stops at the cap either way.
export async function readSource(url: string, cap = SOURCE_VIEW_BYTES): Promise<Source> {
  const response = await fetch(url, { headers: { Range: `bytes=0-${String(cap - 1)}` } });
  if (response.status !== OK && response.status !== PARTIAL_CONTENT) {
    throw new SourceError(response.status);
  }
  const read =
    response.body === null ? { text: "", more: false } : await readUpTo(response.body, cap);
  const partial =
    response.status === PARTIAL_CONTENT && moreAfter(response.headers.get("content-range"));
  return { text: read.text, more: read.more || partial };
}

// The text with every bidi control written out as its code point, so
// what a sender hid behind one stands in the open.
export function revealControls(text: string): string {
  return text.replace(
    BIDI_CONTROLS,
    (control) => `\\u{${(control.codePointAt(0) ?? 0).toString(HEX).toUpperCase()}}`,
  );
}
