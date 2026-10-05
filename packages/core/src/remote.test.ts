// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import * as fc from "fast-check";
import { expect, test } from "vitest";

import { classifyImageUrl, classifyLinkUrl, remoteImageUrl } from "./remote";

const OWN = "mail.example.test";
const DROPPED = { kind: "dropped" };
const WEB_SCHEMES = new Set(["http:", "https:"]);

// Whether the policy's answer for a link is one the frame may carry:
// nothing, an address to write to or a page on another host, off the
// API and without user information.
function carried(value: string): boolean {
  const target = classifyLinkUrl(value, OWN);
  if (target.kind === "dropped") {
    return true;
  }
  const url = new URL(target.url);
  if (target.kind === "mail") {
    return url.protocol === "mailto:";
  }
  const bare = url.username === "" && url.password === "";
  const elsewhere = url.hostname !== OWN && !url.pathname.startsWith("/api/");
  return WEB_SCHEMES.has(url.protocol) && bare && elsewhere;
}

test.each([
  ["https://cdn.example/a.png", "https://cdn.example/a.png"],
  ["http://cdn.example/a.png", "http://cdn.example/a.png"],
  ["HTTPS://CDN.example/a.png?x=1#f", "https://cdn.example/a.png?x=1#f"],
  ["https://cdn.example:8443/a.png", "https://cdn.example:8443/a.png"],
])("an image on another host is remote: %s", (value, url) => {
  expect(classifyImageUrl(value, OWN)).toEqual({ kind: "remote", url });
});

test.each([
  "/api/remote-image?url=https%3A%2F%2Fevil.example%2Fp.gif",
  "logo.png",
  "//cdn.example/a.png",
  "#top",
  "",
  "https://mail.example.test/logo.png",
  "https://MAIL.example.test:8443/logo.png",
  "https://cdn.example/api/x.png",
  "https://mail.example.test/api/jmap/a1/download/u1/b1/x.png?type=image/png",
])("a relative URL, the instance's own host and an /api/ path are dropped: %s", (value) => {
  expect(classifyImageUrl(value, OWN)).toEqual(DROPPED);
});

test.each([
  "javascript:top.__x=1",
  "data:text/html,<script>top.__x=1</script>",
  "data:,x",
  "file:///etc/passwd",
  "mailto:sanne@example.test",
  "blob:https://cdn.example/1",
  "ftp://cdn.example/a.png",
  "https://",
])("every scheme but cid, http, https and a data image is dropped: %s", (value) => {
  expect(classifyImageUrl(value, OWN)).toEqual(DROPPED);
});

test("a data image stays, whatever the case of its type", () => {
  expect(classifyImageUrl("data:image/png;base64,iVBORw0KGgo=", OWN)).toEqual({ kind: "data" });
  expect(classifyImageUrl("DATA:IMAGE/GIF;base64,R0lGOD", OWN)).toEqual({ kind: "data" });
});

test("a cid URL names the part by its decoded Content-ID (RFC 2392 section 2)", () => {
  expect(classifyImageUrl("cid:part1@shop.example", OWN)).toEqual({
    kind: "cid",
    cid: "part1@shop.example",
  });
  expect(classifyImageUrl("cid:a%2Bb%40shop.example", OWN)).toEqual({
    kind: "cid",
    cid: "a+b@shop.example",
  });
  expect(classifyImageUrl("cid:%E0%A4%A", OWN)).toEqual(DROPPED);
});

test("without a known host the /api/ rule still holds", () => {
  expect(classifyImageUrl("https://mail.example.test/logo.png", null)).toEqual({
    kind: "remote",
    url: "https://mail.example.test/logo.png",
  });
  expect(classifyImageUrl("https://mail.example.test/api/x", null)).toEqual(DROPPED);
});

test.each([
  ["https://shop.example/sale?x=1#top", "https://shop.example/sale?x=1#top"],
  ["HTTP://Shop.example", "http://shop.example/"],
  ["https://mybank.example:pw@evil.example/login", "https://evil.example/login"],
])("a link to another host is a page, without user information: %s", (value, url) => {
  expect(classifyLinkUrl(value, OWN)).toEqual({ kind: "web", url });
});

test("a mailto link is an address to write to", () => {
  expect(classifyLinkUrl("mailto:sanne@example.test?subject=hi", OWN)).toEqual({
    kind: "mail",
    url: "mailto:sanne@example.test?subject=hi",
  });
});

test.each([
  "login",
  "/settings",
  "//evil.example/x",
  "#top",
  "",
  "https://mail.example.test/settings",
  "https://shop.example/api/x",
  "javascript:top.__x=1",
  " javascript:top.__x=1",
  "JAVASCRIPT:top.__x=1",
  "data:text/html;base64,PHNjcmlwdD50b3AuX194PTE8L3NjcmlwdD4=",
  "cid:part1@shop.example",
  "file:///etc/passwd",
  "blob:https://shop.example/1",
  "tel:+31201234567",
])("a link the frame may not carry is dropped: %s", (value) => {
  expect(classifyLinkUrl(value, OWN)).toEqual(DROPPED);
});

test("a link of any characters is dropped or one the frame may carry and never throws", () => {
  const scheme = fc.constantFrom("https://", "http://", "mailto:", "javascript:", "data:", "//");
  const host = fc.constantFrom(OWN, "shop.example", "user:pw@shop.example", `x@${OWN}`, "");
  const built = fc.tuple(scheme, host, fc.constantFrom("", "/", "/api/x", "/a/../api/x"));
  const anything = fc.oneof(
    fc.string({ unit: "binary" }),
    fc.webUrl({ withQueryParameters: true, withFragments: true }),
    built.map((pieces) => pieces.join("")),
  );
  fc.assert(
    fc.property(anything, (value) => {
      expect(carried(value)).toBe(true);
    }),
  );
});

test("a remote image loads through the server's route with its URL as one parameter", () => {
  expect(remoteImageUrl("https://cdn.example/a.png?x=1&y=2#f")).toBe(
    "/api/remote-image?url=https%3A%2F%2Fcdn.example%2Fa.png%3Fx%3D1%26y%3D2%23f",
  );
  expect(classifyImageUrl(remoteImageUrl("https://cdn.example/a.png"), OWN)).toEqual(DROPPED);
});
