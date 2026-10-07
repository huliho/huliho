// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { SOURCE_VIEW_BYTES, SourceError, readSource, revealControls } from "./source";

const URL = "/api/jmap/acc-1/download/u1/e-3/message.eml?type=message%2Frfc822";
// A cap small enough to read past in a test.
const CAP = 64;
const PARTIAL_CONTENT = 206;
const MIB = 1024 * 1024;
// What a decoder writes for bytes outside UTF-8.
const REPLACEMENT = String.fromCodePoint(0xff_fd);
// A right-to-left override and a left-to-right isolate.
const OVERRIDE = 0x20_2e;
const ISOLATE = 0x20_66;

// What the stubbed route was asked and whether its stream was let go.
interface Asked {
  headers: Headers;
  letGo: boolean;
}

// A stream of the chunks given, which notes when the reader lets it go.
function streamOf(chunks: readonly Uint8Array[], asked: Asked): ReadableStream<Uint8Array> {
  let at = 0;
  return new ReadableStream({
    pull(controller) {
      const chunk = chunks.at(at);
      at += 1;
      if (chunk === undefined) {
        controller.close();
      } else {
        controller.enqueue(chunk);
      }
    },
    cancel() {
      asked.letGo = true;
    },
  });
}

function bytes(text: string): Uint8Array {
  return new TextEncoder().encode(text);
}

interface Answer {
  status: number;
  chunks: readonly Uint8Array[];
  headers?: Record<string, string>;
}

// A route that answers the one request with the status and the chunks.
function stubRoute(answer: Answer): Asked {
  const asked: Asked = { headers: new Headers(), letGo: false };
  vi.stubGlobal("fetch", (_input: RequestInfo | URL, init?: RequestInit) => {
    asked.headers = new Headers(init?.headers);
    const body = answer.chunks.length === 0 ? null : streamOf(answer.chunks, asked);
    return Promise.resolve(
      new Response(body, { status: answer.status, headers: answer.headers ?? {} }),
    );
  });
  return asked;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("the cap is half a mebibyte", () => {
  expect(SOURCE_VIEW_BYTES).toBe(MIB / 2);
});

test("the read asks for the first bytes from zero and answers the whole of a short message", async () => {
  const asked = stubRoute({ status: 200, chunks: [bytes("From: a@b\r\n\r\n"), bytes("hello")] });
  const source = await readSource(URL, CAP);
  expect(asked.headers.get("range")).toBe(`bytes=0-${String(CAP - 1)}`);
  expect(source).toEqual({ text: "From: a@b\r\n\r\nhello", more: false });
  expect(asked.letGo).toBe(false);
});

test("a whole answer past the cap stops at the cap, lets the stream go and says there is more", async () => {
  const asked = stubRoute({
    status: 200,
    chunks: [bytes("x".repeat(CAP - 10)), bytes("y".repeat(20)), bytes("never read")],
  });
  const source = await readSource(URL, CAP);
  expect(source.text).toBe(`${"x".repeat(CAP - 10)}${"y".repeat(10)}`);
  expect(source.more).toBe(true);
  expect(asked.letGo).toBe(true);
});

test("a partial answer says there is more by its range, and one without a range as well", async () => {
  stubRoute({
    status: PARTIAL_CONTENT,
    chunks: [bytes("a".repeat(CAP))],
    headers: { "content-range": `bytes 0-${String(CAP - 1)}/${String(CAP * 3)}` },
  });
  expect(await readSource(URL, CAP)).toEqual({ text: "a".repeat(CAP), more: true });
  stubRoute({
    status: PARTIAL_CONTENT,
    chunks: [bytes("a".repeat(CAP))],
    headers: { "content-range": `bytes 0-${String(CAP - 1)}/${String(CAP)}` },
  });
  expect((await readSource(URL, CAP)).more).toBe(false);
  stubRoute({ status: PARTIAL_CONTENT, chunks: [bytes("a")] });
  expect((await readSource(URL, CAP)).more).toBe(true);
});

test("bytes outside UTF-8 read as the replacement character, a character the cap cuts included", async () => {
  stubRoute({ status: 200, chunks: [new Uint8Array([0x61, 0xff, 0x62])] });
  expect((await readSource(URL, CAP)).text).toBe(`a${REPLACEMENT}b`);
  stubRoute({ status: 200, chunks: [bytes(`${"a".repeat(CAP - 1)}é`)] });
  const cut = await readSource(URL, CAP);
  expect(cut.text).toBe(`${"a".repeat(CAP - 1)}${REPLACEMENT}`);
  expect(cut.more).toBe(true);
});

test("any other answer is a fault that names the status; an empty answer is an empty source", async () => {
  stubRoute({ status: 404, chunks: [] });
  await expect(readSource(URL, CAP)).rejects.toMatchObject({ name: "SourceError", status: 404 });
  expect(new SourceError(502).message).toBe("the source answered 502");
  stubRoute({ status: 200, chunks: [] });
  expect(await readSource(URL, CAP)).toEqual({ text: "", more: false });
});

test("the bidi controls a sender can turn text around with are written out", () => {
  const hidden = `From: a${String.fromCodePoint(OVERRIDE)}b${String.fromCodePoint(ISOLATE)}c`;
  expect(revealControls(hidden)).toBe("From: a\\u{202E}b\\u{2066}c");
  expect(revealControls("plain")).toBe("plain");
});
