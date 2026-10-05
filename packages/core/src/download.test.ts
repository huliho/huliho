// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import * as fc from "fast-check";
import { expect, test } from "vitest";

import { downloadUrl } from "./download";

// A value in its place: unreserved characters and percent triplets
// alone (RFC 6570 section 3.2.2).
const PLACE = /^(?:[\w.~-]|%[0-9A-F][0-9A-F])*$/;

const SOURCE = {
  template: "/api/jmap/acc-1/download/{accountId}/{blobId}/{name}?type={type}",
  accountId: "u1",
};

test("a blob's URL is the template with its four variables filled in", () => {
  const url = downloadUrl(SOURCE, { blobId: "b7", name: "photo.png", type: "image/png" });
  expect(url).toBe("/api/jmap/acc-1/download/u1/b7/photo.png?type=image%2Fpng");
});

test("a value keeps the unreserved characters alone (RFC 6570 section 3.2.2)", () => {
  const url = downloadUrl(SOURCE, {
    blobId: "e1-1_2",
    name: "a b/c?d#e&f=g(1)!*'~.txt",
    type: "application/octet-stream",
  });
  expect(url).toBe(
    "/api/jmap/acc-1/download/u1/e1-1_2/a%20b%2Fc%3Fd%23e%26f%3Dg%281%29%21%2A%27~.txt" +
      "?type=application%2Foctet-stream",
  );
});

test("a name outside ASCII travels as UTF-8 and a lone surrogate never throws", () => {
  const named = downloadUrl(SOURCE, { blobId: "b", name: "überweisung.pdf", type: "x/y" });
  expect(named).toContain("/%C3%BCberweisung.pdf?");
  const broken = downloadUrl(SOURCE, { blobId: "b", name: "a\uD800b", type: "x/y" });
  expect(broken).toContain("/a%EF%BF%BDb?");
});

test("a name of one or two dots keeps its place in the path (RFC 3986 section 5.2.4)", () => {
  for (const name of [".", ".."]) {
    const url = downloadUrl(SOURCE, { blobId: "b7", name, type: "image/png" });
    expect(url).toBe("/api/jmap/acc-1/download/u1/b7/_?type=image%2Fpng");
    expect(new URL(url, "https://mail.example.test").pathname).toContain("/b7/");
  }
  expect(downloadUrl(SOURCE, { blobId: "b7", name: "...", type: "x" })).toContain("/b7/...?");
});

// What a value reads back as: itself, well formed; one or two dots as
// their stand-in.
function carried(value: string): string {
  return value === "." || value === ".." ? "_" : value.toWellFormed();
}

test("values of any characters fill their own places alone, also once a URL parser read them", () => {
  const anything = fc.oneof(fc.string({ unit: "binary" }), fc.constantFrom(".", "..", "", "%2e"));
  fc.assert(
    fc.property(anything, anything, anything, anything, (accountId, blobId, name, type) => {
      const template = "/d/{accountId}/{blobId}/{name}?type={type}";
      const url = downloadUrl({ template, accountId }, { blobId, name, type });
      const places = url.slice("/d/".length).split(/\/|\?type=/);
      expect(places.every((place) => PLACE.test(place))).toBe(true);
      expect(places.map((place) => decodeURIComponent(place))).toEqual(
        [accountId, blobId, name, type].map((value) => carried(value)),
      );
      const read = new URL(url, "https://mail.example.test");
      expect(read.pathname + read.search).toBe(url);
    }),
  );
});

test("a variable the template names beside the four stays as written", () => {
  const template = "/api/jmap/acc-1/download/{accountId}/{blobId}/{name}?type={type}&x={other}";
  const url = downloadUrl({ ...SOURCE, template }, { blobId: "b", name: "n", type: "t" });
  expect(url).toBe("/api/jmap/acc-1/download/u1/b/n?type=t&x={other}");
});
