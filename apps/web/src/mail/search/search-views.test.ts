// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { MethodFailure, SERVER_UNAVAILABLE } from "@huliho/core";
import type { ListPage, SearchParams } from "@huliho/core";
import { expect, test } from "vitest";

import { INBOX_PAGE, MAILBOXES } from "../fixtures";
import { STAND_IN, countAt, searchViews } from "./search-views";
import type { ViewedSearch } from "./search-views";

const RAN: SearchParams = { q: "offerte" };
const EMPTY: ListPage = { rows: [], total: 0, pending: 0 };

function search(more: Partial<ViewedSearch> = {}): ViewedSearch {
  return {
    key: "k",
    canceled: false,
    phase: "quiet",
    page: { data: undefined, error: null },
    ...more,
  };
}

function views(viewed: ViewedSearch, ran = RAN, online = true) {
  return searchViews({ search: viewed, ran, online, mailboxes: MAILBOXES, locale: "en" });
}

test("before the first run the pane shows the suggestions and says nothing", () => {
  expect(views(search({ key: null }), {})).toEqual({
    slot: { kind: "none" },
    body: { kind: "nothing" },
    sentence: "",
  });
});

test("a search in flight shows the still rows, then the progress, then the slow sentence", () => {
  expect(views(search())).toEqual({
    slot: { kind: "none" },
    body: { kind: "loading" },
    sentence: "",
  });
  expect(views(search({ phase: "searching" }))).toMatchObject({
    slot: { kind: "searching" },
    sentence: "Searching…",
  });
  expect(views(search({ phase: "slow" }))).toMatchObject({
    slot: { kind: "slow" },
    body: { kind: "loading" },
    sentence: "Still searching. A server without a search index can take a while.",
  });
});

test("a canceled search keeps the field and says so in the slot", () => {
  expect(views(search({ canceled: true, phase: "slow" }))).toEqual({
    slot: { kind: "canceled" },
    body: { kind: "nothing" },
    sentence: "Search canceled.",
  });
});

test("offline the body says the search needs a connection", () => {
  expect(views(search(), RAN, false)).toEqual({
    slot: { kind: "none" },
    body: { kind: "offline" },
    sentence: "Search needs a connection.",
  });
});

test("results put the count and the scope in the slot, with the widening", () => {
  const data = { ...INBOX_PAGE, total: 1204 };
  expect(views(search({ page: { data, error: null } }))).toEqual({
    slot: {
      kind: "results",
      sentence: "1,204 results for “offerte” in all mailboxes",
      count: { text: "1,204", at: 0 },
      widen: "Include Junk and Trash",
    },
    body: { kind: "nothing" },
    sentence: "1,204 results for “offerte” in all mailboxes",
  });
  const everywhere = views(search({ page: { data, error: null } }), { ...RAN, in: "all" });
  expect(everywhere.slot).toMatchObject({ widen: null });
});

test("the count is found past a query and a mailbox name that hold its digits", () => {
  const same = { q: "3", mailbox: "" };
  const after = `Results for “${STAND_IN.q}”: 3`;
  expect(countAt(after, "3", same)).toBe("Results for “3”: 3".lastIndexOf("3"));
  expect(countAt(`3 results for “${STAND_IN.q}”`, "3", same)).toBe(0);
  expect(countAt(`Results for “${STAND_IN.q}”`, "3", same)).toBeNull();
  const filled = { q: "dakkapel 3", mailbox: "Q3" };
  const shape = `Results for “${STAND_IN.q}” in ${STAND_IN.mailbox}: 3`;
  const sentence = `Results for “${filled.q}” in ${filled.mailbox}: 3`;
  expect(countAt(shape, "3", filled)).toBe(sentence.lastIndexOf("3"));
});

test("a server without a total gets the results sentence without a count", () => {
  const data = { ...INBOX_PAGE, total: null };
  expect(views(search({ page: { data, error: null } }), { in: "mb-facturen" }).slot).toEqual({
    kind: "results",
    sentence: "Results in Facturen",
    count: null,
    widen: "Search all mailboxes",
  });
});

test("no match shows the empty state, which the status region speaks", () => {
  const empty = views(search({ page: { data: EMPTY, error: null } }));
  expect(empty.slot).toEqual({ kind: "none" });
  expect(empty.body).toEqual({
    kind: "empty",
    words: {
      title: "No mail matches “offerte”",
      hint: "Check the spelling or include Junk and Trash.",
      widen: "Include Junk and Trash",
    },
  });
  expect(empty.sentence).toBe("No mail matches “offerte”");
});

test("the deadline reads its own sentence, any other failure the plain one", () => {
  const late = new MethodFailure(SERVER_UNAVAILABLE, "Email/query");
  expect(views(search({ page: { data: undefined, error: late } }))).toEqual({
    slot: { kind: "none" },
    body: { kind: "failed", deadline: true },
    sentence: "",
  });
  const broken = new MethodFailure("serverFail", "Email/query");
  expect(views(search({ page: { data: undefined, error: broken } })).body).toEqual({
    kind: "failed",
    deadline: false,
  });
  expect(views(search({ page: { data: undefined, error: new TypeError("x") } })).body).toEqual({
    kind: "failed",
    deadline: false,
  });
});
