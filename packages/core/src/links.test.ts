// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import * as fc from "fast-check";
import { expect, test } from "vitest";

import { linkRisk } from "./links";

const OWN = "mail.example.test";

const KINDS = new Set(["own", "mismatch", "international"]);

function risk(target: string, text: string) {
  return linkRisk(new URL(target), text, OWN);
}

test.each([
  ["https://shop.example/sale", "See the sale"],
  ["https://shop.example/sale", ""],
  ["https://shop.example/sale", "shop.example"],
  ["https://shop.example/sale", "https://shop.example/other"],
  ["https://www.shop.example/sale", "shop.example"],
  ["https://shop.example/sale", "www.shop.example/sale?x=1"],
  ["https://shop.example/sale", "shop.example."],
  ["https://shop.example:8443/", "SHOP.example:8443"],
  ["https://shop.example/", "version 1.2"],
  ["https://shop.example/", "v1.2"],
  ["https://shop.example/", "e.g."],
  ["https://shop.example/", "info@mybank.example"],
  ["https://shop.example/", "https://"],
])("a plain link asks nothing: %s shown as %j", (target, text) => {
  expect(risk(target, text)).toBeNull();
});

test.each([
  ["mybank.example", "mybank-secure.example.net"],
  ["https://mybank.example/login", "mybank-secure.example.net"],
  ["  mybank.example/login  ", "mybank-secure.example.net"],
  ["login.mybank.example", "mybank.example"],
  ["HTTP://MYBANK.EXAMPLE", "mybank.example.net"],
])("a text that names another host than the link opens is told: %s", (text, host) => {
  expect(risk(`https://${host}/verify`, text)).toEqual({
    kind: "mismatch",
    text: text.trim(),
    host,
  });
});

test("a text whose address hides its host behind user information names what a reader sees", () => {
  expect(risk("https://evil.example/", "https://mybank.example@evil.example/")).toEqual({
    kind: "mismatch",
    text: "https://mybank.example@evil.example/",
    host: "evil.example",
  });
});

test("a host in the ASCII form of an internationalized name is told", () => {
  expect(risk("https://xn--mybnk-fsa.example/inloggen", "Sign in")).toEqual({
    kind: "international",
    host: "xn--mybnk-fsa.example",
  });
  expect(risk("https://mybänk.example/", "Sign in")).toEqual({
    kind: "international",
    host: "xn--mybnk-ira.example",
  });
  expect(risk("https://shop.xn--p1ai/", "shop.xn--p1ai")).toEqual({
    kind: "international",
    host: "shop.xn--p1ai",
  });
});

test("a text that names another host wins over the internationalized name", () => {
  expect(risk("https://xn--mybnk-fsa.example/", "mybank.example")).toMatchObject({
    kind: "mismatch",
  });
});

test("a link at the app's own host is told first, with its path", () => {
  expect(risk("https://mail.example.test/settings/accounts?x=1", "shop.example")).toEqual({
    kind: "own",
    path: "/settings/accounts",
  });
  expect(risk("http://mail.example.test:8080/", "x")).toEqual({ kind: "own", path: "/" });
});

test("a text past the bound or with a space names no host", () => {
  expect(risk("https://shop.example/", `mybank.example/${"a".repeat(2048)}`)).toBeNull();
  expect(risk("https://shop.example/", "mybank.example is safe")).toBeNull();
});

const NAME = "mybank.example";

test.each([
  ["a zero width joiner inside the name", 0x200d, 2],
  ["a left-to-right mark behind it", 0x200e, NAME.length],
  ["a right-to-left override in front of it", 0x202e, 0],
  ["a byte order mark inside it", 0xfeff, 2],
  ["a soft hyphen inside it", 0xad, 2],
])("a host name stays one with %s, which a reader never sees", (_what, code, at) => {
  const text = NAME.slice(0, at) + String.fromCodePoint(code) + NAME.slice(at);
  expect(text).not.toBe(NAME);
  expect(risk("https://evil.example/", text)).toEqual({
    kind: "mismatch",
    text: "mybank.example",
    host: "evil.example",
  });
  expect(risk("https://mybank.example/", text)).toBeNull();
});

test("a text of any characters answers a risk or none and never throws", () => {
  fc.assert(
    fc.property(fc.string({ unit: "binary" }), fc.webUrl(), (text, target) => {
      const answer = risk(target, text);
      expect(answer === null || KINDS.has(answer.kind)).toBe(true);
    }),
  );
});

test("a text that is the host the link opens never names another one", () => {
  fc.assert(
    fc.property(fc.domain(), (host) => {
      expect(risk(`https://${host}/`, host)?.kind).not.toBe("mismatch");
      expect(risk(`https://${host}/`, `https://www.${host}/path`)?.kind).not.toBe("mismatch");
    }),
  );
});
