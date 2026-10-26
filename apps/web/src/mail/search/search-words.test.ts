// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { FIXED_NOW, MAILBOXES } from "../fixtures";
import { emptyWords, resultsWords, scopeWords, tokenWords, widened } from "./search-words";
import type { WordContext } from "./search-words";

const NAMES = new Map([["pieter@blom-installaties.example", "Pieter Blom"]]);
const EN: WordContext = { locale: "en", mailboxes: MAILBOXES, names: NAMES, today: FIXED_NOW };
const NL: WordContext = { ...EN, locale: "nl" };

test("a token shows its operator in English and its value as a person reads it", () => {
  expect(tokenWords({ key: "from", value: "Pieter@Blom-Installaties.example" }, EN)).toEqual({
    operator: "from:",
    value: "Pieter Blom",
  });
  expect(tokenWords({ key: "from", value: '"Blom Installatie"' }, EN).value).toBe(
    "Blom Installatie",
  );
  expect(tokenWords({ key: "from", value: "jonas" }, EN).value).toBe("jonas");
  expect(tokenWords({ key: "in", value: "mb-offertes" }, NL)).toEqual({
    operator: "in:",
    value: "Offertes",
  });
  expect(tokenWords({ key: "in", value: "all" }, EN).value).toBe("anywhere");
  expect(tokenWords({ key: "unread" }, NL)).toEqual({ operator: "is:", value: "unread" });
  expect(tokenWords({ key: "attachment" }, EN)).toEqual({ operator: "has:", value: "attachment" });
});

test("a day reads in the locale, with the year where it is not this one", () => {
  expect(tokenWords({ key: "after", value: "2026-04-01" }, EN).value).toBe("Apr 1");
  expect(tokenWords({ key: "after", value: "2026-04-01" }, NL).value).toBe("1 apr");
  expect(tokenWords({ key: "before", value: "2025-12-24" }, EN).value).toBe("Dec 24, 2025");
});

test("the scope reads as the default, one mailbox by its name or all of them", () => {
  expect(scopeWords({ q: "x" }, MAILBOXES)).toEqual({ scope: "default", mailbox: "" });
  expect(scopeWords({ in: "mb-facturen" }, MAILBOXES)).toEqual({
    scope: "one",
    mailbox: "Facturen",
  });
  expect(scopeWords({ in: "all" }, MAILBOXES)).toEqual({ scope: "all", mailbox: "" });
  expect(scopeWords({ in: "mb-gone" }, MAILBOXES)).toEqual({ scope: "default", mailbox: "" });
});

test("the results header counts in the locale, with or without a text", () => {
  const all = scopeWords({ in: "all" }, MAILBOXES);
  const one = scopeWords({ in: "mb-facturen" }, MAILBOXES);
  expect(resultsWords({ q: "offerte" }, 1, one, "en")).toBe("1 result for “offerte” in Facturen");
  expect(resultsWords({ q: "offerte" }, 1204, all, "en")).toBe(
    "1,204 results for “offerte” in all mailboxes, Junk and Trash included",
  );
  expect(resultsWords({ unread: true }, 2, one, "nl")).toBe("2 resultaten in Facturen");
  expect(resultsWords({ q: "offerte" }, null, one, "en")).toBe("Results for “offerte” in Facturen");
});

function scope(params: { in?: string }) {
  return scopeWords(params, MAILBOXES);
}

test("the empty state offers the widening the scope allows", () => {
  expect(emptyWords({ q: "zzz" }, scope({}), "en")).toEqual({
    title: "No mail matches “zzz”",
    hint: "Check the spelling or include Junk and Trash.",
    widen: "Include Junk and Trash",
  });
  expect(emptyWords({ unread: true }, scope({ in: "mb-facturen" }), "en")).toEqual({
    title: "No mail matches these filters",
    hint: "Check the spelling or search all mailboxes.",
    widen: "Search all mailboxes",
  });
  expect(emptyWords({ q: "zzz" }, scope({ in: "all" }), "en")).toMatchObject({
    hint: "Check the spelling.",
    widen: null,
  });
});

test("the widening drops one mailbox for the default and adds Junk and Trash to the default", () => {
  const one = scopeWords({ in: "mb-facturen" }, MAILBOXES);
  const plain = scopeWords({}, MAILBOXES);
  expect(widened({ q: "x", in: "mb-facturen" }, one)).toEqual({ q: "x" });
  expect(widened({ q: "x" }, plain)).toEqual({ q: "x", in: "all" });
});
