// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import * as fc from "fast-check";
import { expect, test } from "vitest";

import { alters, patched, replayed, unpatched } from "./patch";
import type { EmailPatch, PendingRow } from "./store";

const KEYWORDS = ["$seen", "$flagged", "$answered", "a/b", "c~d"];

// The path a /set update names a keyword with (RFC 6901 section 3).
function pathOf(keyword: string): string {
  return `keywords/${keyword.replaceAll("~", "~0").replaceAll("/", "~1")}`;
}

function rowOf(seq: number, patch: EmailPatch): PendingRow {
  return { seq, type: "Email", id: "e1", patch, inverse: {}, sentAt: null };
}

const keywords = fc
  .uniqueArray(fc.constantFrom(...KEYWORDS))
  .map((held): Record<string, true> => Object.fromEntries(held.map((keyword) => [keyword, true])));

const patch = fc
  .uniqueArray(fc.tuple(fc.constantFrom(...KEYWORDS), fc.constantFrom(true, null)), {
    minLength: 1,
    selector: ([keyword]) => keyword,
  })
  .map((entries): EmailPatch =>
    Object.fromEntries(entries.map(([keyword, value]) => [pathOf(keyword), value])),
  );

test("a patch sets a keyword with true and takes it off with null; its inverse says what stood there", () => {
  const read = patched({ $flagged: true }, { "keywords/$seen": true, "keywords/$flagged": null });
  expect(read.keywords).toEqual({ $seen: true });
  expect(read.inverse).toEqual({ "keywords/$seen": null, "keywords/$flagged": true });
});

test("a path names its keyword with the escapes of a pointer token (RFC 6901 section 3)", () => {
  expect(patched({}, { "keywords/a~1b": true, "keywords/c~0d": true }).keywords).toEqual({
    "a/b": true,
    "c~d": true,
  });
  expect(patched({ "a/b": true }, { "keywords/a~1b": null }).keywords).toEqual({});
});

test("a path that names no keyword is refused", () => {
  expect(() => patched({}, { "mailboxIds/inbox": true })).toThrow("keywords alone");
  expect(() => patched({}, { "keywords/": true })).toThrow("keywords alone");
  expect(() => patched({}, { keywords: true })).toThrow("keywords alone");
});

test("a patch alters the keywords only when some path says something new", () => {
  expect(alters({ "keywords/$seen": true }, { "keywords/$seen": null })).toBe(true);
  expect(alters({ "keywords/$seen": true }, { "keywords/$seen": true })).toBe(false);
  expect(alters({ "keywords/$seen": null }, { "keywords/$seen": null })).toBe(false);
});

test("taking the pending rows back from what they made restores the keywords they started on", () => {
  fc.assert(
    fc.property(keywords, fc.array(patch, { maxLength: 4 }), (held, patches) => {
      const rows = patches.map((one, index) => rowOf(index + 1, one));
      const replay = replayed(held, rows);
      expect(unpatched(replay.keywords, replay.rows)).toEqual(held);
    }),
  );
});

test("a patch laid twice is the patch laid once", () => {
  fc.assert(
    fc.property(keywords, patch, (held, one) => {
      const once = patched(held, one).keywords;
      expect(patched(once, one).keywords).toEqual(once);
    }),
  );
});
