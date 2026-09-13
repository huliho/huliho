// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { fitsAddress, fitsHostName } from "./address";

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
