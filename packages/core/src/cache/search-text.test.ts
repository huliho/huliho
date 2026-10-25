// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import * as fc from "fast-check";
import { expect, test } from "vitest";

import { mailbox } from "./fake-jmap";
import type { SearchParams } from "./search";
import { parseQuery, queryOf } from "./search-text";
import type { QueryContext } from "./search-text";

function named(id: string, name: string, sortOrder = 10) {
  return { ...mailbox(id), name, sortOrder };
}

const MAILBOXES = [
  { ...mailbox("inbox", "inbox"), name: "Inbox" },
  { ...mailbox("junk", "junk"), name: "Junk" },
  named("work", "Work"),
  named("px", "Project X"),
  // Two folders of one name: the tree draws the lower sortOrder first.
  named("reports-b", "Reports", 20),
  named("reports-a", "Reports", 15),
  named("quoted", 'Say "hi"'),
];

const EN: QueryContext = { mailboxes: MAILBOXES, locale: "en" };
const NL: QueryContext = { mailboxes: MAILBOXES, locale: "nl" };

const word = fc.stringMatching(/^[a-z0-9]{1,8}$/u);
const phrase = fc
  .array(word, { minLength: 1, maxLength: 3 })
  .map((words) => `"${words.join(" ")}"`);
// Text that looks like an operator yet stays text.
const lookalike = fc.constantFrom(
  "label:work",
  "re:hi",
  "is:read",
  "has:pdf",
  "in:nowhere",
  "after:yesterday",
  "before:2026-02-30",
  "from:",
);
const terms = (term: fc.Arbitrary<string>): fc.Arbitrary<string> =>
  fc.array(term, { minLength: 1, maxLength: 4 }).map((parts) => parts.join(" "));
const day = fc
  .date({ min: new Date(2000, 0, 1), max: new Date(2099, 11, 31), noInvalidDate: true })
  .map((date) => date.toISOString().slice(0, 10));

// Text that an operator claims as typed.
const operator = fc.constantFrom(
  "from:sanne",
  "in:work",
  "in:anywhere",
  "is:unread",
  "has:attachment",
  "after:2026-10-01",
);

// Params whose `q` is built from the given pieces.
function paramsWith(piece: fc.Arbitrary<string>): fc.Arbitrary<SearchParams> {
  return fc.record(
    {
      q: terms(piece),
      in: fc.constantFrom("all", ...MAILBOXES.map((row) => row.id)),
      from: terms(fc.oneof(word, phrase, fc.constant("sanne@example.test"))),
      after: day,
      before: day,
      unread: fc.constant(true as const),
      starred: fc.constant(true as const),
      attachment: fc.constant(true as const),
    },
    { requiredKeys: [] },
  );
}

const paramsArbitrary = paramsWith(fc.oneof(word, phrase, lookalike));

test("every params value round-trips through the text and back, in either locale", () => {
  fc.assert(
    fc.property(paramsArbitrary, fc.constantFrom(EN, NL), (params, context) => {
      expect(parseQuery(queryOf(params, context), context)).toEqual(params);
    }),
  );
});

test("a `q` that holds an operator reads back with the same operators and a text that is a fixed point", () => {
  fc.assert(
    fc.property(
      paramsWith(fc.oneof(word, phrase, lookalike, operator)),
      fc.constantFrom(EN, NL),
      (params, context) => {
        const text = queryOf(params, context);
        const read = parseQuery(text, context);
        expect({ ...read, q: undefined }).toEqual({ ...params, q: undefined });
        expect(queryOf(read, context)).toBe(text);
      },
    ),
  );
});

test("the text quotes a piece of `q` that an operator would claim, so it reads back as text", () => {
  const text = queryOf({ q: 'from:sanne in:work invoice from:"de Vries"' }, EN);
  expect(text).toBe('"from:sanne" "in:work" invoice "from:de Vries"');
  expect(parseQuery(text, EN)).toEqual({ q: text });
});

test("a quoted run is never an operator", () => {
  expect(parseQuery('"from:sanne" "is:unread" report', EN)).toEqual({
    q: '"from:sanne" "is:unread" report',
  });
});

test("an unknown operator stays text, so a server's own operators reach it as typed", () => {
  expect(parseQuery("label:work invoice is:read", EN)).toEqual({
    q: "label:work invoice is:read",
  });
});

test("a date that does not parse stays text, and so does an operator without a value", () => {
  expect(parseQuery("after:2026-02-30 before:yesterday after:2026-1-5x", EN)).toEqual({
    q: "after:2026-02-30 before:yesterday after:2026-1-5x",
  });
  expect(parseQuery("from: sanne in: is:", EN)).toEqual({ q: "from: sanne in: is:" });
});

test("the operator words read in any case, and the text keeps one space between its pieces", () => {
  expect(parseQuery("  FROM:sanne   Is:Unread  quarterly\treport HAS:Attachment ", EN)).toEqual({
    from: "sanne",
    unread: true,
    attachment: true,
    q: "quarterly report",
  });
});

test("`in:` takes a name in any case, an id or `anywhere`; a name the tree lacks stays text; the last one wins", () => {
  expect(parseQuery("in:work", EN)).toEqual({ in: "work" });
  expect(parseQuery("in:WORK", EN)).toEqual({ in: "work" });
  expect(parseQuery('in:"project x"', EN)).toEqual({ in: "px" });
  expect(parseQuery("in:px", EN)).toEqual({ in: "px" });
  expect(parseQuery("in:Anywhere", EN)).toEqual({ in: "all" });
  expect(parseQuery("in:nowhere invoice", EN)).toEqual({ q: "in:nowhere invoice" });
  expect(parseQuery("in:junk invoice in:inbox", EN)).toEqual({ in: "inbox", q: "invoice" });
});

test("a name that repeats reads as the first in the tree's order, so the text names the other by its id", () => {
  expect(parseQuery("in:reports", EN)).toEqual({ in: "reports-a" });
  expect(queryOf({ in: "reports-a" }, EN)).toBe("in:Reports");
  expect(queryOf({ in: "reports-b" }, EN)).toBe("in:reports-b");
  expect(queryOf({ in: "px" }, EN)).toBe('in:"Project X"');
  expect(queryOf({ in: "quoted" }, EN)).toBe("in:quoted");
  expect(queryOf({ in: "all" }, EN)).toBe("in:anywhere");
});

test("a date reads as ISO 8601, as Gmail writes it and as the locale writes it in digits", () => {
  expect(parseQuery("after:2026-10-01 before:2026/10/25", EN)).toEqual({
    after: "2026-10-01",
    before: "2026-10-25",
  });
  expect(parseQuery("after:10/1/2026 before:2026-1-5", EN)).toEqual({
    after: "2026-10-01",
    before: "2026-01-05",
  });
  expect(parseQuery("after:1-10-2026 before:25.10.2026", NL)).toEqual({
    after: "2026-10-01",
    before: "2026-10-25",
  });
  // A two-digit year says nothing certain.
  expect(parseQuery("after:10/1/26", EN)).toEqual({ q: "after:10/1/26" });
});

test("two senders both count, and the text writes the operators first", () => {
  expect(parseQuery('from:sanne from:"de Vries" invoice', EN)).toEqual({
    from: 'sanne "de Vries"',
    q: "invoice",
  });
  expect(queryOf({ q: "invoice", from: 'sanne "de Vries"', unread: true }, EN)).toBe(
    'from:sanne from:"de Vries" is:unread invoice',
  );
});
