// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { FIXED_NOW, MAILBOXES, SENDERS } from "../fixtures";
import { presetDay, senderPrefix, suggestionsOf } from "./search-suggestions";
import type { Recent, SuggestionGroup, SuggestionInput } from "./search-suggestions";

const RECENTS: Recent[] = [
  { key: "r1", params: { q: "offerte" }, text: "offerte" },
  { key: "r2", params: { q: "factuur", unread: true }, text: "is:unread factuur" },
];

function input(text: string, more: Partial<SuggestionInput> = {}): SuggestionInput {
  return {
    text,
    senders: [],
    mailboxes: MAILBOXES,
    recents: RECENTS,
    today: FIXED_NOW,
    locale: "en",
    ...more,
  };
}

// The groups by their names, each with its rows' values.
function values(groups: readonly SuggestionGroup[]): Record<string, string[]> {
  return Object.fromEntries(
    groups.map((group) => [group.value, group.items.map((item) => item.value)]),
  );
}

test("an empty field offers the recents alone, newest first", () => {
  expect(values(suggestionsOf(input("")))).toEqual({ recent: ["recent:r1", "recent:r2"] });
  expect(suggestionsOf(input("", { recents: [] }))).toEqual([]);
});

test("a word offers the plain search first, then the senders and the mailboxes it starts", () => {
  const groups = suggestionsOf(input("off", { senders: SENDERS.slice(0, 1) }));
  expect(values(groups)).toEqual({
    suggest: ["run", "from:pieter@blom-installaties.example", "in:mb-offertes"],
    recent: ["recent:r1"],
  });
  expect(groups[0]?.items[0]).toEqual({ kind: "run", value: "run", text: "off" });
});

test("a mailbox matches from the start of any word of its name, without its marks", () => {
  const school = MAILBOXES.filter((row) => row.id === "mb-facturen").map((row) =>
    Object.assign(structuredClone(row), { id: "mb-ecole", name: "Projet École" }),
  );
  const tree = [...MAILBOXES.filter((row) => row.id !== "mb-facturen"), ...school];
  expect(values(suggestionsOf(input("ecol", { mailboxes: tree, recents: [] })))).toEqual({
    suggest: ["run", "in:mb-ecole"],
  });
});

test("from: offers the senders the worker answered", () => {
  expect(values(suggestionsOf(input("from:pie", { senders: SENDERS, recents: [] })))).toEqual({
    suggest: SENDERS.map((sender) => `from:${sender.email}`),
  });
});

test("in: offers the mailboxes in the tree's order with the widening under its rule", () => {
  expect(values(suggestionsOf(input("in:", { recents: [] })))).toEqual({
    suggest: [
      "in:mb-inbox",
      "in:mb-drafts",
      "in:mb-sent",
      "in:mb-archive",
      "in:mb-junk",
      "in:mb-trash",
      "in:mb-facturen",
      "in:mb-verbouwing",
      "in:mb-offertes",
    ],
    anywhere: ["in:anywhere"],
  });
  expect(values(suggestionsOf(input("in:any", { recents: [] })))).toEqual({
    anywhere: ["in:anywhere"],
  });
  expect(values(suggestionsOf(input("in:fac", { recents: [] })))).toEqual({
    suggest: ["in:mb-facturen"],
  });
});

test("after: and before: offer the three presets with the day each starts on", () => {
  const groups = suggestionsOf(input("after:", { recents: [] }));
  expect(groups[0]?.items).toEqual([
    {
      kind: "token",
      value: "after:week",
      token: { key: "after", value: "2026-05-07" },
      preset: "week",
    },
    {
      kind: "token",
      value: "after:month",
      token: { key: "after", value: "2026-04-14" },
      preset: "month",
    },
    {
      kind: "token",
      value: "after:year",
      token: { key: "after", value: "2025-05-14" },
      preset: "year",
    },
  ]);
  expect(values(suggestionsOf(input("before:", { recents: [] })))["suggest"]).toEqual([
    "before:week",
    "before:month",
    "before:year",
  ]);
});

test("is: and has: offer the flags their value starts", () => {
  const plain = { recents: [] };
  expect(values(suggestionsOf(input("is:", plain)))).toEqual({
    suggest: ["flag:unread", "flag:starred"],
  });
  expect(values(suggestionsOf(input("is:S", plain)))).toEqual({ suggest: ["flag:starred"] });
  expect(values(suggestionsOf(input("has:", plain)))).toEqual({ suggest: ["flag:attachment"] });
  expect(suggestionsOf(input("is:read", plain))).toEqual([]);
});

test("an operator the parser leaves as text gets the plain search", () => {
  expect(values(suggestionsOf(input("offerte label:werk", { recents: [] })))).toEqual({
    suggest: ["run"],
  });
});

test("the recents follow what the field holds, in any case and without marks", () => {
  expect(values(suggestionsOf(input("FACT", { recents: RECENTS })))["recent"]).toEqual([
    "recent:r2",
  ]);
  expect(values(suggestionsOf(input("unread", { recents: RECENTS })))["recent"]).toEqual([
    "recent:r2",
  ]);
});

test("a preset a month back from a long month lands on the shorter month's last day", () => {
  expect(presetDay("month", new Date(2026, 2, 31))).toBe("2026-02-28");
  expect(presetDay("month", new Date(2028, 2, 31))).toBe("2028-02-29");
  expect(presetDay("year", new Date(2028, 1, 29))).toBe("2027-02-28");
  expect(presetDay("week", new Date(2026, 0, 3))).toBe("2025-12-27");
  expect(presetDay("month", new Date(2026, 0, 15))).toBe("2025-12-15");
});

test("the worker is asked for senders after from: or for a plain word, nothing else", () => {
  expect(senderPrefix("")).toBeNull();
  expect(senderPrefix("offerte ")).toBeNull();
  expect(senderPrefix("offerte pie")).toBe("pie");
  expect(senderPrefix("from:")).toBe("");
  expect(senderPrefix("FROM:Pie")).toBe("Pie");
  expect(senderPrefix("in:fac")).toBeNull();
});
