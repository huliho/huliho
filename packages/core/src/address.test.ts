// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { asciiAddress, fitsAddress, fitsHostName, sameAddress } from "./address";

// The 256-octet path of RFC 5321 section 4.5.3.1.3 minus its brackets.
const MAX_BYTES = 254;
const DOMAIN = "@example.test";

test("a plain address passes, with the domain in any case or script", () => {
  expect(fitsAddress("sanne@example.test")).toBe(true);
  expect(fitsAddress("Sanne@Example.TEST")).toBe(true);
  expect(fitsAddress("sanne@bücher.example")).toBe(true);
  expect(fitsAddress(`${"a".repeat(MAX_BYTES - DOMAIN.length)}${DOMAIN}`)).toBe(true);
});

test.each([
  "",
  "sanne",
  "sanne@",
  "@example.test",
  "sanne@localhost",
  "sanne@127.0.0.1",
  "sanne@[::1]",
  "sanne@2130706433",
  "sanne@exa mple.test",
  "sanne\n@example.test",
  "sanne@example.test.",
  "sanne@-example.test",
  "sanne@ex_ample.test",
  "sanne@a..b",
  "sa@nne@example.test",
  "sanne@%41.test",
  `${"a".repeat(MAX_BYTES - DOMAIN.length + 1)}${DOMAIN}`,
])("%j is refused the way the server refuses it", (address) => {
  expect(fitsAddress(address)).toBe(false);
});

test.each([
  '"sanne de vries"@example.test',
  "a>,<evil@example.test",
  "sänne@example.test",
  ".sanne@example.test",
  "sanne.@example.test",
  "san..ne@example.test",
  "san(ne)@example.test",
  "san[ne]@example.test",
  "san;ne@example.test",
  "san\\ne@example.test",
])("%j is no address a header carries", (address) => {
  expect(asciiAddress(address)).toBeNull();
});

test("a local part outside ASCII passes the server's reading and stays out of a header", () => {
  expect(fitsAddress("sänne@example.test")).toBe(true);
  expect(fitsAddress('"sanne de vries"@example.test')).toBe(false);
});

test("a header's local part is a dot-atom of the atext of RFC 5322 section 3.2.3", () => {
  expect(asciiAddress("o'brien+tag@example.test")).toBe("o'brien+tag@example.test");
  expect(asciiAddress("a.b-c_d!#$%&*/=?^`{|}~@example.test")).toBe(
    "a.b-c_d!#$%&*/=?^`{|}~@example.test",
  );
});

test("an address leaves with its domain as the ASCII name a header carries, rfc5322_2_2", () => {
  expect(asciiAddress("Sanne@Bücher.Example")).toBe("Sanne@xn--bcher-kva.example");
  expect(asciiAddress("sanne@example.test")).toBe("sanne@example.test");
  expect(asciiAddress("sänne@example.test")).toBeNull();
});

test("the bound counts the address in the form a header carries", () => {
  const local = "a".repeat(MAX_BYTES - "@xn--bcher-kva.example".length);
  expect(asciiAddress(`${local}@bücher.example`)).toBe(`${local}@xn--bcher-kva.example`);
  expect(asciiAddress(`a${local}@bücher.example`)).toBeNull();
});

test("two spellings of one address compare equal in their ASCII form", () => {
  expect(sameAddress("Sanne@Bücher.example", "sanne@xn--bcher-kva.example")).toBe(true);
  expect(sameAddress("sanne@example.test", "mo@example.test")).toBe(false);
  expect(sameAddress("not an address", "NOT AN ADDRESS")).toBe(true);
});

test("a host name is a name in any case or script, one label being enough", () => {
  expect(fitsHostName("imap.example.test")).toBe(true);
  expect(fitsHostName("IMAP.Bücher.example")).toBe(true);
  expect(fitsHostName("localhost")).toBe(true);
});

test.each([
  "",
  "127.0.0.1",
  "[::1]",
  "2130706433",
  "mail.example.test:993",
  "https://mail.example.test",
  "mail.example.test/",
  "user@mail.example.test",
  "mail example.test",
  "-mail.example.test",
])("%j is no host name: an address literal or a URL part", (text) => {
  expect(fitsHostName(text)).toBe(false);
});
