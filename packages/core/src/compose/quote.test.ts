// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import type { EmailBody } from "../cache/store";
import type { EmailBodyPart } from "../jmap/body";
import { writeFlowed } from "./flowed-write";
import {
  attributionOf,
  forwardHtml,
  forwardLines,
  forwardedParts,
  quotedParts,
  replyHtml,
  replyLines,
} from "./quote";

const START = String.fromCodePoint(0x20_68);
const END = String.fromCodePoint(0x20_69);

function part(
  partId: string | null,
  type: string,
  extra: Partial<EmailBodyPart> = {},
): EmailBodyPart {
  return {
    partId,
    blobId: partId === null ? null : `e1-${partId}`,
    size: 100,
    name: null,
    type,
    charset: null,
    disposition: null,
    cid: null,
    language: null,
    location: null,
    ...extra,
  };
}

function body(overrides: Partial<EmailBody> = {}): EmailBody {
  return {
    id: "e1",
    bodyStructure: part(null, "multipart/alternative"),
    textBody: [part("1", "text/plain")],
    htmlBody: [part("2", "text/html")],
    attachments: [],
    bodyValues: {
      "1": { value: "Hello\n> earlier\n", isEncodingProblem: false, isTruncated: false },
    },
    authentication: { status: "absent" },
    flowed: null,
    large: false,
    fetchedAt: 0,
    bytes: 0,
    ...overrides,
  };
}

test("the attribution names the sender isolated and the date it was sent", () => {
  expect(
    attributionOf({
      from: [{ name: "Mo", email: "mo@x.test" }],
      sentAt: "2026-01-02T03:04:05+01:00",
      receivedAt: "2026-01-02T02:05:00Z",
    }),
  ).toEqual({ date: "2026-01-02T03:04:05+01:00", name: `${START}Mo${END}` });
});

test("without a name the address stands in; without a sent date the arrival", () => {
  expect(
    attributionOf({
      from: [{ name: " ", email: "mo@x.test" }],
      sentAt: null,
      receivedAt: "2026-01-02T02:05:00Z",
    }),
  ).toEqual({
    date: "2026-01-02T02:05:00Z",
    name: `${START}mo@x.test${END}`,
  });
});

test("a reply's HTML quote escapes the attribution and keeps the parent's HTML in a cite blockquote", () => {
  expect(replyHtml('On <date>, "Mo" & co wrote:', "<p>Hi</p>")).toBe(
    '<p>On &lt;date&gt;, &quot;Mo&quot; &amp; co wrote:</p><blockquote type="cite"><p>Hi</p></blockquote>',
  );
});

test("a reply's text quote is the parent's paragraphs one depth deeper under the attribution", () => {
  expect(replyLines("On Monday, Mo wrote:", body())).toEqual([
    { depth: 0, text: "On Monday, Mo wrote:" },
    { depth: 1, text: "Hello" },
    { depth: 2, text: "earlier" },
  ]);
});

test("a flowed parent is joined and quoted again at its own depth plus one, rfc3676_4_5", () => {
  const flowed = writeFlowed([
    { depth: 0, text: "word ".repeat(30).trim() },
    { depth: 1, text: "older" },
  ]);
  const parent = body({
    flowed: { delSp: true },
    bodyValues: { "1": { value: flowed, isEncodingProblem: false, isTruncated: false } },
  });
  expect(replyLines("A:", parent)).toEqual([
    { depth: 0, text: "A:" },
    { depth: 1, text: "word ".repeat(30).trim() },
    { depth: 2, text: "older" },
  ]);
});

test("a parent without a text part leaves the text quote to its HTML", () => {
  const html = body({ textBody: [part("2", "text/html")], bodyValues: {} });
  expect(replyLines("A:", html)).toBeNull();
  expect(
    forwardLines(
      { title: "T", from: "F", date: "D", to: "To", subject: "S" },
      { from: "", date: "", to: "", subject: "" },
      html,
    ),
  ).toBeNull();
});

const WORDS = {
  title: "Forwarded message",
  from: "From",
  date: "Date",
  to: "To",
  subject: "Subject",
};
const FACTS = {
  from: "Mo <mo@x.test>",
  date: "2 January 2026",
  to: "Sanne <s@x.test>",
  subject: "Lunch & more",
};

test("a forward opens with its header and keeps the parent's paragraphs at their depth", () => {
  expect(forwardLines(WORDS, FACTS, body())).toEqual([
    { depth: 0, text: "-------- Forwarded message --------" },
    { depth: 0, text: "From: Mo <mo@x.test>" },
    { depth: 0, text: "Date: 2 January 2026" },
    { depth: 0, text: "To: Sanne <s@x.test>" },
    { depth: 0, text: "Subject: Lunch & more" },
    { depth: 0, text: "Hello" },
    { depth: 1, text: "earlier" },
  ]);
  expect(forwardHtml(WORDS, FACTS, "<p>Hi</p>")).toBe(
    "<p>-------- Forwarded message --------<br>From: Mo &lt;mo@x.test&gt;<br>Date: 2 January 2026<br>To: Sanne &lt;s@x.test&gt;<br>Subject: Lunch &amp; more</p><p>Hi</p>",
  );
});

const rich = body({
  htmlBody: [
    part("2", "text/html"),
    part("3", "image/png", { cid: "logo@x", disposition: "inline" }),
  ],
  attachments: [
    part("3", "image/png", { cid: "logo@x", disposition: "inline" }),
    part("4", "application/pdf", { name: "plan.pdf", disposition: "attachment" }),
    part("5", "image/gif", { cid: "unused@x", disposition: "inline" }),
  ],
});
const HTML = '<p><img src="cid:logo@x"><img src="https://remote.test/a.png"></p>';

test("a reply carries the pictures the HTML names by cid as inline parts, each once, rfc2392", () => {
  expect(quotedParts(rich, HTML)).toEqual([
    {
      blobId: "e1-3",
      name: null,
      type: "image/png",
      size: 100,
      cid: "logo@x",
      disposition: "inline",
    },
  ]);
});

function pictures(cids: readonly string[]): EmailBody {
  return body({
    attachments: cids.map((cid, index) =>
      part(String(index + 3), "image/png", { cid, disposition: "inline" }),
    ),
  });
}

test("a part whose cid opens another's is carried only where the HTML names it whole", () => {
  const parent = pictures(["image1", "image10"]);
  const named = quotedParts(parent, '<img src="cid:image10">');
  expect(named.map((file) => file.cid)).toEqual(["image10"]);
  expect(
    quotedParts(parent, "<p style=\"background:url(cid:image1)\"><img src='cid:image10'>"),
  ).toHaveLength(2);
});

test("a percent-encoded reference carries its part; one that does not decode reads as written", () => {
  const parent = pictures(["my logo@x", "100%@x"]);
  const named = quotedParts(parent, '<img src="cid:my%20logo%40x"><img src="cid:100%@x">');
  expect(named.map((file) => file.cid)).toEqual(["my logo@x", "100%@x"]);
});

test("a forward carries the named pictures inline and every other attachment as an attachment", () => {
  expect(forwardedParts(rich, HTML)).toEqual([
    {
      blobId: "e1-3",
      name: null,
      type: "image/png",
      size: 100,
      cid: "logo@x",
      disposition: "inline",
    },
    {
      blobId: "e1-4",
      name: "plan.pdf",
      type: "application/pdf",
      size: 100,
      cid: null,
      disposition: "attachment",
    },
    {
      blobId: "e1-5",
      name: null,
      type: "image/gif",
      size: 100,
      cid: null,
      disposition: "attachment",
    },
  ]);
});
