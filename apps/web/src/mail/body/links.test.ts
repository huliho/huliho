// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { readFragment } from "../../open/fragment";
import { OWN_HOST } from "./frame-rig";
import { rewriteLinks } from "./links";
import { purify } from "./profile";

const POLICY = { ownHost: OWN_HOST, linkKey: "key-1" };

function links(html: string): HTMLAnchorElement[] {
  const body = purify(html);
  rewriteLinks(body, POLICY);
  return Array.from(body.querySelectorAll("a"));
}

function only(html: string): HTMLAnchorElement {
  const [link] = links(html);
  if (link === undefined) {
    throw new Error("the fixture holds no link");
  }
  return link;
}

test("a link opens through the app's own route, its target, text and key in the fragment", () => {
  const link = only('<a href="https://shop.example/sale?x=1&y=2#top">See the <b>sale</b></a>');
  const href = link.getAttribute("href") ?? "";
  expect(href.startsWith("/open#")).toBe(true);
  expect(href).not.toContain("?");
  expect(readFragment(href.slice("/open".length))).toEqual({
    target: "https://shop.example/sale?x=1&y=2#top",
    text: "See the sale",
    key: "key-1",
  });
  expect(link.getAttribute("target")).toBe("_blank");
  expect(link.getAttribute("rel")).toBe("noopener noreferrer");
});

test("the title shows where the link leads, whatever title the sender wrote", () => {
  const link = only('<a href="https://evil.example/login" title="https://mybank.example/">x</a>');
  expect(link.getAttribute("title")).toBe("https://evil.example/login");
});

test("a mail address opens through the route as well", () => {
  const link = only('<a href="mailto:sanne@example.test?subject=hi">mail</a>');
  expect(readFragment((link.getAttribute("href") ?? "").slice("/open".length))?.target).toBe(
    "mailto:sanne@example.test?subject=hi",
  );
});

test("user information in front of a host leaves the target", () => {
  const link = only('<a href="https://mybank.example@evil.example/login">x</a>');
  expect(link.getAttribute("title")).toBe("https://evil.example/login");
});

test.each([
  '<a href="login">x</a>',
  '<a href="/settings">x</a>',
  '<a href="//evil.example/x">x</a>',
  '<a href="#top">x</a>',
  '<a href="https://mail.example.test/settings">x</a>',
  '<a href="https://shop.example/api/x">x</a>',
  '<a href="cid:part1@shop.example">x</a>',
  "<a>x</a>",
])("a link the policy drops stays as its text: %s", (html) => {
  const link = only(html);
  expect(link.textContent).toBe("x");
  expect(link.getAttributeNames()).toEqual([]);
});

test("a link too long to carry loses its text first and then opens nothing", () => {
  const long = `https://shop.example/${"a".repeat(3900)}`;
  const carried = only(`<a href="${long}">${"word ".repeat(60)}</a>`);
  const fragment = readFragment((carried.getAttribute("href") ?? "").slice("/open".length));
  expect(fragment).toEqual({ target: long, text: null, key: "key-1" });
  const dropped = only(`<a href="https://shop.example/${"a".repeat(4100)}">x</a>`);
  expect(dropped.hasAttribute("href")).toBe(false);
  expect(dropped.textContent).toBe("x");
});

test("every link of a body is rewritten, each with its own text", () => {
  const all = links('<a href="https://a.example/">a</a><p><a href="https://b.example/">b</a></p>');
  expect(all.map((link) => link.getAttribute("title"))).toEqual([
    "https://a.example/",
    "https://b.example/",
  ]);
});
