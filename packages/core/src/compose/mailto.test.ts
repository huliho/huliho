// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import * as fc from "fast-check";
import { expect, test } from "vitest";

import { fitsId } from "./headers";
import { MAILTO_MAX_CHARS, readMailto } from "./mailto";

test("every safe field fills the draft, the body's line breaks as line feeds, rfc6068_2", () => {
  const draft = readMailto(
    "mailto:mo@x.test,%22Doe%2C%20Jane%22%20%3Cjane@x.test%3E?cc=bo@x.test&bcc=ali@x.test" +
      "&subject=Lunch%20%26%20more&body=Hi%2C%0D%0Asee%20you%0Athere&in-reply-to=%3Cp1@x.test%3E",
  );
  expect(draft).toEqual({
    to: [
      { name: null, email: "mo@x.test" },
      { name: "Doe, Jane", email: "jane@x.test" },
    ],
    cc: [{ name: null, email: "bo@x.test" }],
    bcc: [{ name: null, email: "ali@x.test" }],
    subject: "Lunch & more",
    body: "Hi,\nsee you\nthere",
    inReplyTo: ["p1@x.test"],
  });
});

test("a repeated field is joined and a field name reads in any case", () => {
  const draft = readMailto("MAILTO:?To=a@x.test&to=b@x.test&SUBJECT=one&subject=two&body=x&body=y");
  expect(draft?.to.map((token) => ("email" in token ? token.email : token.text))).toEqual([
    "a@x.test",
    "b@x.test",
  ]);
  expect(draft?.subject).toBe("one two");
  expect(draft?.body).toBe("x\ny");
});

test("every other field is ignored, a sender and a header among them, rfc6068_7", () => {
  const draft = readMailto(
    "mailto:mo@x.test?from=evil@x.test&reply-to=evil@x.test&x-mailer=z&content-type=text/html",
  );
  expect(draft).toEqual({
    to: [{ name: null, email: "mo@x.test" }],
    cc: [],
    bcc: [],
    subject: "",
    body: "",
    inReplyTo: [],
  });
});

test("a subject never carries a line break and a field that does not decode is left out", () => {
  const draft = readMailto("mailto:mo@x.test?subject=a%0D%0Ab&cc=%E0%A4%A&body=%ZZ");
  expect(draft?.subject).toBe("a b");
  expect(draft?.cc).toEqual([]);
  expect(draft?.body).toBe("");
});

test("in-reply-to keeps the ids a header can carry", () => {
  const draft = readMailto("mailto:mo@x.test?in-reply-to=%3Cp1@x.test%3E%20%3Cwith%20space@x%3E");
  expect(draft?.inReplyTo).toEqual(["p1@x.test"]);
});

test("text in the address field that is no address stays as text", () => {
  expect(readMailto("mailto:not%20an%20address")?.to).toEqual([{ text: "not an address" }]);
});

test("a plus stays a plus in an address and in a field, rfc6068_5", () => {
  const draft = readMailto("mailto:mo+news@x.test?subject=a+b");
  expect(draft?.to).toEqual([{ name: null, email: "mo+news@x.test" }]);
  expect(draft?.subject).toBe("a+b");
});

const opening = fc.constantFrom(
  "mailto:",
  "MAILTO:",
  "MailTo:",
  "mailt:",
  "https:",
  "",
  " mailto:",
);
// What a URI is made of: its marks, percent escapes whole or cut short
// and the names of fields.
const MARKS = ["a", "é", " ", "+", "?", "&", "=", "%", "<", ">", "@", ",", "\r", "\n", "\0"];
const ESCAPES = ["%0D", "%0A", "%00", "%3C", "%3E", "%20", "%E0", "x.test", "mo@x.test"];
const FIELDS = ["to", "cc", "bcc", "subject", "body", "in-reply-to", "from"];
const rest = fc.string({
  unit: fc.constantFrom(...MARKS, ...ESCAPES, ...FIELDS),
  maxLength: 40,
});
// Nothing, or enough to pass the cap whatever stands in front.
const padding = fc.constantFrom(0, MAILTO_MAX_CHARS);

test("any text opens a draft a header can carry or nothing, the cap and the scheme deciding which", () => {
  fc.assert(
    fc.property(opening, rest, padding, (scheme, text, pad) => {
      const uri = `${scheme}${text}${"a".repeat(pad)}`;
      const draft = readMailto(uri);
      const opens = uri.length <= MAILTO_MAX_CHARS && uri.toLowerCase().startsWith("mailto:");
      expect(draft !== null).toBe(opens);
      expect(draft?.subject ?? "").not.toMatch(/[\r\n\0]/u);
      expect(draft?.body ?? "").not.toMatch(/\r/u);
      expect((draft?.inReplyTo ?? []).every(fitsId)).toBe(true);
    }),
  );
});

test("a URI past the cap or of another scheme opens nothing", () => {
  const long = `mailto:mo@x.test?body=${"a".repeat(MAILTO_MAX_CHARS)}`;
  expect(readMailto(long)).toBeNull();
  expect(readMailto(`mailto:mo@x.test?body=${"a".repeat(MAILTO_MAX_CHARS - 30)}`)).not.toBeNull();
  expect(readMailto("https://x.test")).toBeNull();
  expect(readMailto("mailt:mo@x.test")).toBeNull();
});
