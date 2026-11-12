// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import * as fc from "fast-check";
import { expect, test } from "vitest";

import { draftContent as content } from "../cache/fake-jmap";
import type { DraftAttachment } from "../cache/store";
import { unflow } from "../text/flowed";
import { writeFlowed } from "./flowed-write";
import { HTML_PART, TEXT_PART, createObject, sentType, treeOf } from "./object";
import type { CreatePart, EmailCreate } from "./object";

const DRAFTS = "drafts-box";

function file(
  blobId: string,
  disposition: "attachment" | "inline",
  cid: string | null = null,
): DraftAttachment {
  return { blobId, name: `${blobId}.png`, type: "image/png", size: 10, cid, disposition };
}

// The shape of a tree: each multipart with its children, each leaf by
// what it holds.
function shapeOf(part: CreatePart): unknown {
  if (part.subParts !== undefined) {
    return { [part.type ?? ""]: part.subParts.map(shapeOf) };
  }
  return part.partId ?? `blob:${part.blobId ?? ""}`;
}

const shapes = fc.record({
  rich: fc.boolean(),
  inline: fc.integer({ min: 0, max: 3 }),
  attached: fc.integer({ min: 0, max: 3 }),
  flowedText: fc.boolean(),
});

test("the tree holds each level only where it holds something, for the eight shapes, rfc8621_4_6", () => {
  fc.assert(
    fc.property(shapes, ({ rich, inline, attached, flowedText }) => {
      const inlines = Array.from({ length: inline }, (_, index) =>
        file(`i${String(index)}`, "inline", `c${String(index)}`),
      );
      const files = Array.from({ length: attached }, (_, index) =>
        file(`a${String(index)}`, "attachment"),
      );
      const draft = content({ mode: rich ? "rich" : "plain", attachments: [...files, ...inlines] });
      const alternative = rich ? { "multipart/alternative": [TEXT_PART, HTML_PART] } : TEXT_PART;
      const named = rich ? inlines : [];
      const mixed = rich ? files : [...files, ...inlines];
      const related =
        named.length > 0
          ? { "multipart/related": [alternative, ...named.map((one) => `blob:${one.blobId}`)] }
          : alternative;
      const expected =
        mixed.length > 0
          ? { "multipart/mixed": [related, ...mixed.map((one) => `blob:${one.blobId}`)] }
          : related;
      expect(shapeOf(treeOf(draft, flowedText))).toEqual(expected);
    }),
  );
});

test("a plain draft keeps an inline file as it stands, beside the text", () => {
  const picture = file("i0", "inline", "c0");
  const tree = treeOf(content({ mode: "plain", attachments: [picture] }), true);
  expect(shapeOf(tree)).toEqual({ "multipart/mixed": [TEXT_PART, "blob:i0"] });
  expect(leaves(tree).at(-1)).toMatchObject({ disposition: "inline", cid: "c0" });
});

function leaves(part: CreatePart): CreatePart[] {
  return part.subParts === undefined ? [part] : part.subParts.flatMap(leaves);
}

test("a leaf holds a part id or a blob, never a charset, a size or a transfer encoding", () => {
  fc.assert(
    fc.property(shapes, ({ rich, inline, attached, flowedText }) => {
      const files = [
        ...Array.from({ length: inline }, (_, index) =>
          file(`i${String(index)}`, "inline", `c${String(index)}`),
        ),
        ...Array.from({ length: attached }, (_, index) => file(`a${String(index)}`, "attachment")),
      ];
      const object = createObject(content({ mode: rich ? "rich" : "plain", attachments: files }), {
        draftsId: DRAFTS,
        flowedText,
      });
      const values = object.bodyValues;
      for (const leaf of leaves(object.bodyStructure)) {
        expect((leaf.partId === undefined) !== (leaf.blobId === undefined)).toBe(true);
        expect(Object.keys(leaf)).not.toContain("charset");
        expect(Object.keys(leaf)).not.toContain("size");
        expect(leaf.partId === undefined || leaf.partId in values).toBe(true);
      }
      expect(JSON.stringify(object)).not.toMatch(/Content-Transfer-Encoding/iu);
    }),
  );
});

test("the create files the draft in Drafts alone with $draft and $seen and sets no server property", () => {
  const object = createObject(content(), { draftsId: DRAFTS, flowedText: true });
  expect(object["mailboxIds"]).toEqual({ [DRAFTS]: true });
  expect(object["keywords"]).toEqual({ $draft: true, $seen: true });
  expect(object["from"]).toEqual([{ name: null, email: "sanne@example.test" }]);
  for (const property of [
    "messageId",
    "sentAt",
    "receivedAt",
    "textBody",
    "htmlBody",
    "attachments",
  ]) {
    expect(object).not.toHaveProperty(property);
  }
});

function textOf(object: EmailCreate): string {
  return new Map(Object.entries(object.bodyValues)).get(TEXT_PART)?.value ?? "";
}

test("the text part goes flowed where the server writes the parameters, else as one line per paragraph", () => {
  const long = "word ".repeat(40).trim();
  const draft = content({
    text: writeFlowed([
      { depth: 0, text: long },
      { depth: 1, text: "quoted" },
    ]),
  });
  const flowed = createObject(draft, { draftsId: DRAFTS, flowedText: true });
  const fixed = createObject(draft, { draftsId: DRAFTS, flowedText: false });
  expect(unflow(textOf(flowed), true)).toEqual([
    { depth: 0, text: long },
    { depth: 1, text: "quoted" },
  ]);
  expect(textOf(fixed)).toBe(`${long}\n> quoted\n`);
  expect(leaves(fixed.bodyStructure)[0]).toEqual({
    partId: TEXT_PART,
    type: "text/plain",
  });
  expect(leaves(flowed.bodyStructure)[0]).toEqual({
    partId: TEXT_PART,
    "header:Content-Type": " text/plain; format=flowed; delsp=yes",
  });
});

test("an address leaves in ASCII and text that is no address stays out of the message", () => {
  const object = createObject(
    content({
      to: [{ name: "Jo", email: "jo@bücher.example" }, { text: "not an address" }],
      cc: [{ name: "  ", email: "cc@example.test" }],
      bcc: [{ text: "bob" }],
    }),
    { draftsId: DRAFTS, flowedText: true },
  );
  expect(object["to"]).toEqual([{ name: "Jo", email: "jo@xn--bcher-kva.example" }]);
  expect(object["cc"]).toEqual([{ name: null, email: "cc@example.test" }]);
  expect(object).not.toHaveProperty("bcc");
  const stale = createObject(content({ to: [{ name: "Jo", email: "jo@localhost" }] }), {
    draftsId: DRAFTS,
    flowedText: true,
  });
  expect(stale).not.toHaveProperty("to");
});

test("a file with a cid that is no inline part stays an attachment", () => {
  const tagged = file("a0", "attachment", "c0");
  const tree = treeOf(content({ attachments: [tagged] }), true);
  expect(shapeOf(tree)).toEqual({
    "multipart/mixed": [{ "multipart/alternative": [TEXT_PART, HTML_PART] }, "blob:a0"],
  });
  expect(leaves(tree).at(-1)).toMatchObject({ disposition: "attachment", cid: "c0" });
});

test("an inline file whose cid no header can carry goes beside the others without one", () => {
  const picture = file("i0", "inline", "a b");
  const tree = treeOf(content({ attachments: [picture] }), true);
  expect(shapeOf(tree)).toEqual({
    "multipart/mixed": [{ "multipart/alternative": [TEXT_PART, HTML_PART] }, "blob:i0"],
  });
  expect(leaves(tree).at(-1)).not.toHaveProperty("cid");
});

// Two tokens in lower case around one slash (RFC 2045 section 5.1), each
// within the 127 bytes of RFC 6838 section 4.2.
const SENT_TYPE = /^[a-z0-9!#$%&'*+.^_`|~-]{1,127}\/[a-z0-9!#$%&'*+.^_`|~-]{1,127}$/u;
// An id of at most 512 bytes: printable ASCII with no space or angle bracket.
const SENT_ID = /^[!-;=?-~]{1,512}$/u;
// Past the bound of an id and of a token alike.
const LONG = "a".repeat(600);

const hostileValue = fc.oneof(
  fc.string({
    unit: fc.constantFrom("a", "B", "/", " ", "<", ">", "é", "語", "\r", "\n", "\0", "+", ";", "@"),
    maxLength: 12,
  }),
  fc.constantFrom("", "text", "a/b/c", "IMAGE/PNG", "image/p ng", LONG, `${LONG}/b`, `a/${LONG}`),
  fc.constantFrom("multipart/related", "Multipart/Mixed", "message/rfc822"),
);

const hostileFile = fc.record({
  type: hostileValue,
  cid: fc.option(hostileValue),
  disposition: fc.constantFrom("attachment" as const, "inline" as const),
});

test("every blob leaf goes with a type of two tokens and a cid only where it is an id, whatever the file held", () => {
  fc.assert(
    fc.property(fc.array(hostileFile, { maxLength: 4 }), fc.boolean(), (held, rich) => {
      const files = held.map((one, index) => ({ ...file(`b${String(index)}`, "inline"), ...one }));
      const draft = content({ mode: rich ? "rich" : "plain", attachments: files });
      const blobs = leaves(treeOf(draft, true)).filter((leaf) => leaf.blobId !== undefined);
      expect(blobs).toHaveLength(files.length);
      expect(new Set(blobs.map((leaf) => leaf.blobId))).toEqual(
        new Set(files.map((one) => one.blobId)),
      );
      for (const leaf of blobs) {
        expect(leaf.type).toMatch(SENT_TYPE);
        expect(leaf.type).not.toMatch(/^multipart\//u);
      }
      for (const cid of blobs.flatMap((leaf) => leaf.cid ?? [])) {
        expect(cid).toMatch(SENT_ID);
      }
    }),
  );
});

test("a type goes in lower case and one that is not two tokens goes as plain bytes", () => {
  expect(sentType({ type: "IMAGE/PNG" })).toBe("image/png");
  expect(sentType({ type: "application/vnd.x+json" })).toBe("application/vnd.x+json");
  for (const type of ["", "text", "a/b/c", "image/p ng", "imäge/png", `${LONG}/png`]) {
    expect(sentType({ type })).toBe("application/octet-stream");
  }
});

test("a multipart type in any case goes as plain bytes and a message type keeps its own", () => {
  expect(sentType({ type: "multipart/related" })).toBe("application/octet-stream");
  expect(sentType({ type: "Multipart/Mixed" })).toBe("application/octet-stream");
  expect(sentType({ type: "message/rfc822" })).toBe("message/rfc822");
});

const hostile = fc.string({
  unit: fc.constantFrom("a", "é", "\r", "\n", "\0", " ", "語", "=?"),
  maxLength: 40,
});

test("no header value carries CR, LF or NUL and no body value a NUL, whatever the composer held", () => {
  fc.assert(
    fc.property(hostile, hostile, hostile, hostile, (subject, name, body, attachment) => {
      const object = createObject(
        content({
          subject,
          to: [{ name, email: "mo@example.test" }],
          identity: { id: "i1", name, email: "sanne@example.test" },
          text: body,
          html: body,
          attachments: [{ ...file("a1", "attachment"), name: attachment }],
        }),
        { draftsId: DRAFTS, flowedText: true },
      );
      const { bodyValues, ...headers } = object;
      expect(JSON.stringify(headers)).not.toMatch(/\\[rn]|\\u0000/u);
      expect(JSON.stringify(bodyValues)).not.toMatch(/\\u0000/u);
    }),
  );
});

test("a subject outside ASCII goes as it is for the server to encode; an empty one goes not at all", () => {
  expect(
    createObject(content({ subject: " Grüße 語 " }), { draftsId: DRAFTS, flowedText: true })[
      "subject"
    ],
  ).toBe("Grüße 語");
  expect(
    createObject(content({ subject: "  " }), { draftsId: DRAFTS, flowedText: true }),
  ).not.toHaveProperty("subject");
});

test("the reply headers carry the ids a header can hold", () => {
  const object = createObject(
    content({ inReplyTo: ["p@x.test"], references: ["a@x.test", "bad id", "p@x.test"] }),
    {
      draftsId: DRAFTS,
      flowedText: true,
    },
  );
  expect(object["inReplyTo"]).toEqual(["p@x.test"]);
  expect(object["references"]).toEqual(["a@x.test", "p@x.test"]);
});
