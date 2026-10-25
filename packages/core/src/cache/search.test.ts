// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import * as fc from "fast-check";
import { expect, test } from "vitest";

import type { EmailFilter } from "../jmap/calls";
import { mailbox } from "./fake-jmap";
import { inScope } from "./scope";
import type { Scope } from "./scope";
import { readSearchParams, scopeOf, searchFilterOf, searchKeyOf, searchParamsOf } from "./search";
import type { SearchParams } from "./search";
import type { MemberState } from "./store";

const MAILBOXES = [
  mailbox("inbox", "inbox"),
  mailbox("archive", "archive"),
  mailbox("junk", "junk"),
  mailbox("trash", "trash"),
  mailbox("work"),
];

// The scope a condition covers, read back from its mailbox conditions.
function scopeOfFilter(filter: EmailFilter): Scope {
  if (filter.inMailbox !== undefined) {
    return { kind: "one", mailboxId: filter.inMailbox };
  }
  if (filter.inMailboxOtherThan !== undefined) {
    return { kind: "except", mailboxIds: filter.inMailboxOtherThan };
  }
  return { kind: "all" };
}

// Local midnight of a day, as the device's zone has it.
function midnight(year: number, month: number, day: number): number {
  return new Date(year, month - 1, day).getTime();
}

function state(...mailboxIds: string[]): MemberState {
  return {
    keywords: {},
    mailboxIds: Object.fromEntries(mailboxIds.map((id) => [id, true as const])),
  };
}

const textArbitrary = fc
  .string({ minLength: 1, maxLength: 12 })
  .filter((text) => text.trim() === text && text !== "");
const dayArbitrary = fc
  .date({ min: new Date(2000, 0, 1), max: new Date(2099, 11, 31), noInvalidDate: true })
  .map((date) => date.toISOString().slice(0, 10));
const paramsArbitrary: fc.Arbitrary<SearchParams> = fc.record(
  {
    q: textArbitrary,
    in: fc.constantFrom("inbox", "archive", "junk", "trash", "work", "all"),
    from: textArbitrary,
    after: dayArbitrary,
    before: dayArbitrary,
    unread: fc.constant(true as const),
    starred: fc.constant(true as const),
    attachment: fc.constant(true as const),
  },
  { requiredKeys: [] },
);

test("a param that fails its check reads as absent and anything else is left out", () => {
  expect(
    readSearchParams({
      q: "   ",
      in: "",
      from: 7,
      after: "2026-02-30",
      before: "25-10-2026",
      unread: "yes",
      starred: true,
      attachment: 1,
      box: "inbox",
      label: "work",
    }),
  ).toEqual({});
  expect(readSearchParams(null)).toEqual({});
  expect(readSearchParams("q=invoice")).toEqual({});
  expect(readSearchParams({ q: "  invoice  ", unread: "1", after: "2026-02-28" })).toEqual({
    q: "invoice",
    unread: true,
    after: "2026-02-28",
  });
});

test("a key that is no JSON holds no params, and one with more than the params holds the params alone", () => {
  expect(searchParamsOf("q=invoice")).toEqual({});
  expect(searchParamsOf("")).toEqual({});
  expect(searchParamsOf('{"q":"invoice","box":"inbox","__proto__":{"in":"all"}}')).toEqual({
    q: "invoice",
  });
});

test("every params value round-trips through its key, and one search has one key", () => {
  fc.assert(
    fc.property(paramsArbitrary, (params) => {
      expect(searchParamsOf(searchKeyOf(params))).toEqual(params);
      expect(searchKeyOf(searchParamsOf(searchKeyOf(params)))).toBe(searchKeyOf(params));
    }),
  );
  expect(searchKeyOf({ unread: true, q: "invoice" })).toBe(
    searchKeyOf({ q: "invoice", unread: true }),
  );
});

test("every params value builds one condition whose scope reads back (RFC 8621 section 4.4.1)", () => {
  fc.assert(
    fc.property(paramsArbitrary, (params) => {
      const filter = searchFilterOf(params, MAILBOXES);
      expect(scopeOfFilter(filter)).toEqual(scopeOf(params, MAILBOXES));
      expect(filter.text).toBe(params.q);
      expect(filter.from).toBe(params.from);
      expect(filter.hasAttachment).toBe(params.attachment);
      expect(filter.notKeyword).toBe(params.unread === true ? "$seen" : undefined);
      expect(filter.hasKeyword).toBe(params.starred === true ? "$flagged" : undefined);
    }),
  );
});

test("the default scope leaves Junk and Trash out, `all` keeps them and one mailbox narrows", () => {
  expect(searchFilterOf({ q: "invoice" }, MAILBOXES)).toEqual({
    inMailboxOtherThan: ["junk", "trash"],
    text: "invoice",
  });
  expect(searchFilterOf({ q: "invoice", in: "all" }, MAILBOXES)).toEqual({ text: "invoice" });
  expect(searchFilterOf({ in: "junk", unread: true }, MAILBOXES)).toEqual({
    inMailbox: "junk",
    notKeyword: "$seen",
  });
  // An account without Junk or Trash leaves nothing out.
  expect(searchFilterOf({ q: "invoice" }, [mailbox("inbox", "inbox")])).toEqual({
    inMailboxOtherThan: [],
    text: "invoice",
  });
});

test("an `in` that names no mailbox of the account reads as the default scope, never as all", () => {
  expect(scopeOf({ in: "gone" }, MAILBOXES)).toEqual({
    kind: "except",
    mailboxIds: ["junk", "trash"],
  });
});

test("a range runs from the start of the `after` day to the start of the `before` day, in the device's zone (RFC 8621 section 4.4.1)", () => {
  const filter = searchFilterOf({ after: "2026-01-31", before: "2026-02-28" }, MAILBOXES);
  expect(filter.after).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/u);
  expect(filter.before).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/u);
  expect(Date.parse(filter.after ?? "")).toBe(midnight(2026, 1, 31));
  expect(Date.parse(filter.before ?? "")).toBe(midnight(2026, 2, 28));
  const leap = searchFilterOf({ before: "2028-02-29" }, MAILBOXES);
  expect(Date.parse(leap.before ?? "")).toBe(midnight(2028, 2, 29));
});

test("an email stands in a scope when one of its mailboxes is covered", () => {
  const except: Scope = { kind: "except", mailboxIds: ["junk", "trash"] };
  expect(inScope(state("inbox"), except)).toBe(true);
  expect(inScope(state("trash"), except)).toBe(false);
  expect(inScope(state("trash", "work"), except)).toBe(true);
  expect(inScope(state(), { kind: "all" })).toBe(false);
  expect(inScope(state("junk"), { kind: "all" })).toBe(true);
  expect(inScope(state("work"), { kind: "one", mailboxId: "inbox" })).toBe(false);
});
