// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { QueryContext, SearchParams } from "@huliho/core";
import { expect, test } from "vitest";

import { MAILBOXES } from "../fixtures";
import {
  draftOf,
  holdsNothing,
  paramsOf,
  settled,
  tailOf,
  tokensOf,
  withOperator,
  withToken,
  withoutLast,
} from "./search-draft";

const CONTEXT: QueryContext = { mailboxes: MAILBOXES, locale: "en" };

test("the tokens stand in the order the text writes the operators", () => {
  expect(
    tokensOf({
      attachment: true,
      unread: true,
      before: "2026-05-01",
      after: "2026-04-01",
      in: "mb-facturen",
      from: 'pieter "Blom Installatie"',
      q: "offerte",
    }),
  ).toEqual([
    { key: "from", value: "pieter" },
    { key: "from", value: '"Blom Installatie"' },
    { key: "in", value: "mb-facturen" },
    { key: "after", value: "2026-04-01" },
    { key: "before", value: "2026-05-01" },
    { key: "unread" },
    { key: "attachment" },
  ]);
});

test("every search reads back from the draft it shows", () => {
  const searches: SearchParams[] = [
    { q: "offerte" },
    { q: '"dak kapel" offerte' },
    { from: "pieter sanne@fastmail.com", q: "offerte" },
    { in: "mb-offertes", unread: true },
    { in: "all", after: "2026-04-01", before: "2026-05-01", attachment: true },
    { starred: true, q: "in:nowhere" },
  ];
  for (const search of searches) {
    expect(paramsOf(draftOf(search, CONTEXT), CONTEXT)).toEqual(search);
  }
});

test("a search's text keeps a piece an operator would claim quoted, so it stays text", () => {
  const draft = draftOf({ q: "from:pieter offerte" }, CONTEXT);
  expect(draft).toEqual({ operators: {}, text: '"from:pieter" offerte' });
  const once = paramsOf(draft, CONTEXT);
  expect(once).toEqual({ q: '"from:pieter" offerte' });
  expect(paramsOf(draftOf(once, CONTEXT), CONTEXT)).toEqual(once);
});

test("the senders of the tokens and of the text add up", () => {
  expect(paramsOf({ operators: { from: "pieter" }, text: "from:sanne offerte" }, CONTEXT)).toEqual({
    from: "pieter sanne",
    q: "offerte",
  });
});

test("an operator the text completes becomes a token once white space follows it", () => {
  expect(settled({ operators: {}, text: "has:attachment " }, CONTEXT)).toEqual({
    operators: { attachment: true },
    text: "",
  });
  expect(settled({ operators: {}, text: "offerte from:pieter dak" }, CONTEXT)).toEqual({
    operators: { from: "pieter" },
    text: "offerte dak",
  });
  expect(settled({ operators: {}, text: "in:facturen " }, CONTEXT)).toEqual({
    operators: { in: "mb-facturen" },
    text: "",
  });
});

test("the word being typed stays text, and so does a quoted operator", () => {
  const typing = { operators: {}, text: "offerte from:pie" };
  expect(settled(typing, CONTEXT)).toBe(typing);
  const quoted = { operators: {}, text: '"from:pieter" ' };
  expect(settled(quoted, CONTEXT)).toBe(quoted);
  const unknown = { operators: {}, text: "label:werk " };
  expect(settled(unknown, CONTEXT)).toBe(unknown);
});

test("the word being typed starts after the last white space outside a quoted run", () => {
  expect(tailOf("offerte from:pie")).toBe("from:pie");
  expect(tailOf("offerte ")).toBe("");
  expect(tailOf('offerte "dak kap')).toBe('"dak kap');
  expect(tailOf('"dak kapel" pie')).toBe("pie");
});

test("a picked token takes the place of the word being typed", () => {
  const draft = { operators: { unread: true as const }, text: "offerte from:pie" };
  expect(withToken(draft, { key: "from", value: "pieter@blom-installaties.example" })).toEqual({
    operators: { unread: true, from: "pieter@blom-installaties.example" },
    text: "offerte",
  });
});

test("a shortcut's token leaves the text as typed", () => {
  expect(withOperator({ operators: {}, text: "offerte" }, { key: "attachment" })).toEqual({
    operators: { attachment: true },
    text: "offerte",
  });
});

test("Backspace at the start takes the last token as drawn", () => {
  const draft = { operators: { from: "pieter sanne", unread: true as const }, text: "x" };
  const once = withoutLast(draft);
  expect(once).toEqual({ operators: { from: "pieter sanne" }, text: "x" });
  expect(withoutLast(once)).toEqual({ operators: { from: "pieter" }, text: "x" });
  expect(withoutLast({ operators: {}, text: "x" })).toEqual({ operators: {}, text: "x" });
});

test("a search with no text and no operator holds nothing to run", () => {
  expect(holdsNothing({})).toBe(true);
  expect(holdsNothing(paramsOf({ operators: {}, text: "   " }, CONTEXT))).toBe(true);
  expect(holdsNothing({ unread: true })).toBe(false);
});
