// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import * as fc from "fast-check";
import { expect, test } from "vitest";

import { asciiAddress } from "../address";
import { isAddress, tokenize, withTokens } from "./addresses";
import type { MailAddress } from "./headers";

test("a quoted name with a comma stays one token", () => {
  expect(tokenize('"Doe, Jane" <jane@example.test>, bob@example.test')).toEqual([
    { name: "Doe, Jane", email: "jane@example.test" },
    { name: null, email: "bob@example.test" },
  ]);
});

test("a group's members are tokens of their own, rfc5322_3_4", () => {
  expect(tokenize('Team: anna@x.test, "Bo, B" <bo@x.test>;, carl@x.test')).toEqual([
    { name: null, email: "anna@x.test" },
    { name: "Bo, B", email: "bo@x.test" },
    { name: null, email: "carl@x.test" },
  ]);
});

test("a pasted list with semicolons and line breaks is one token per address", () => {
  expect(tokenize("anna@x.test; bob@x.test\r\ncarl@x.test\n\ndirk@x.test").map(isAddress)).toEqual([
    true,
    true,
    true,
    true,
  ]);
});

test("a bare address and an address with a comment read as addresses", () => {
  expect(tokenize("jane@example.test (Jane Doe)")).toEqual([
    { name: "Jane Doe", email: "jane@example.test" },
  ]);
  expect(tokenize("bob@example.test")).toEqual([{ name: null, email: "bob@example.test" }]);
});

test("text that fails the server's reading of an address comes back as text", () => {
  expect(tokenize("not an address, a@b, Jane <jane@bad>, jäne@example.test")).toEqual([
    { text: "not an address" },
    { text: "a@b" },
    { text: "Jane <jane@bad>" },
    { text: "jäne@example.test" },
  ]);
});

test("an address list never reads one address as two", () => {
  expect(tokenize("<a>,<evil@example.test>")).toEqual([
    { text: "a" },
    { name: null, email: "evil@example.test" },
  ]);
});

test("a name carries no control character and empty entries give nothing", () => {
  expect(tokenize('=?utf-8?q?Ja=00ne?= <jane@example.test>, "Mo\tB" <mo@example.test>')).toEqual([
    { name: "Ja ne", email: "jane@example.test" },
    { name: "Mo B", email: "mo@example.test" },
  ]);
  expect(tokenize(" ,  ;, ")).toEqual([]);
  expect(tokenize("")).toEqual([]);
});

// The marks of an address list, the breaks of a paste and pieces of an address.
const MARKS = ["@", ".", ",", ";", ":", '"', "'", "<", ">", "(", ")", "\\"];
const BREAKS = ["a", "B", "é", "語", " ", "\r", "\n", "\0", "\t"];
const PIECES = ["=?utf-8?q?", "?=", "x.test", "jane@example.test"];
const pasted = fc.string({
  unit: fc.constantFrom(...MARKS, ...BREAKS, ...PIECES),
  maxLength: 30,
});

const CONTROL = /\p{Cc}/u;

function written({ name, email }: MailAddress): string {
  return name === null ? email : `"${name.replaceAll(/[\\"]/gu, String.raw`\$&`)}" <${email}>`;
}

test("any text reads as addresses a header carries or as text; the addresses read back the same", () => {
  fc.assert(
    fc.property(pasted, (text) => {
      const tokens = tokenize(text);
      const addresses = tokens.filter(isAddress);
      for (const { name, email } of addresses) {
        expect(asciiAddress(email)).not.toBeNull();
        expect(name ?? "").not.toMatch(CONTROL);
      }
      for (const token of tokens.filter((one) => !isAddress(one))) {
        expect(Object.keys(token)).toEqual(["text"]);
      }
      const again = tokenize(addresses.map(written).join(","));
      expect(again.map((token) => (isAddress(token) ? token.email : token))).toEqual(
        addresses.map((address) => address.email),
      );
    }),
  );
});

test("a token the field holds already is left out, an address in any case", () => {
  const held = tokenize("Jane <jane@example.test>, bob");
  const added = tokenize("JANE@Example.TEST, bob, carl@example.test");
  expect(withTokens(held, added)).toEqual([
    { name: "Jane", email: "jane@example.test" },
    { text: "bob" },
    { name: null, email: "carl@example.test" },
  ]);
});
