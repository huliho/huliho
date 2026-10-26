// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { readSearchParams } from "@huliho/core";
import { expect, test } from "vitest";

import { parseSearch, searchRoute, stringifySearch } from "./route-params";

test("every param stays the string the address writes, so a number is text", () => {
  expect(parseSearch("?q=123&unread=1&box=mb-inbox")).toEqual({
    q: "123",
    unread: "1",
    box: "mb-inbox",
  });
  expect(readSearchParams(searchRoute(parseSearch("?q=123")))).toEqual({ q: "123" });
  expect(parseSearch("?q=%22dak%20kapel%22")).toEqual({ q: '"dak kapel"' });
});

test("the route keeps the known names that hold a string and nothing else", () => {
  expect(
    searchRoute({ q: "offerte", in: 3, from: ["a"], box: "mb-inbox", reconnect: "acc-1" }),
  ).toEqual({ q: "offerte", box: "mb-inbox" });
  expect(searchRoute({ __proto__: "x", constructor: "y" })).toEqual({});
});

test("the address writes the string params alone and nothing for none", () => {
  expect(stringifySearch({ q: "dak kapel", unread: "1", skipped: 1 })).toBe(
    "?q=dak+kapel&unread=1",
  );
  expect(stringifySearch({})).toBe("");
  expect(parseSearch(stringifySearch({ q: "a&b=c" }))).toEqual({ q: "a&b=c" });
});
