// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import * as fc from "fast-check";
import { expect, test } from "vitest";

import { alters, emptied, patched, pathOf, replayed, unpatched } from "./patch";
import type { EmailPatch, MemberState, PendingRow } from "./store";

const KEYWORDS = ["$seen", "$flagged", "$junk", "a/b", "c~d"];
const MAILBOXES = ["inbox", "archive", "trash", "x/y", "z~w"];

function rowOf(seq: number, patch: EmailPatch, destroy = false): PendingRow {
  const row: PendingRow = { seq, type: "Email", id: "e1", patch, inverse: {}, sentAt: null };
  return destroy ? { ...row, destroy: true, held: true } : row;
}

function setOf(names: readonly string[]): Record<string, true> {
  return Object.fromEntries(names.map((name) => [name, true]));
}

const state = fc
  .record({
    keywords: fc.uniqueArray(fc.constantFrom(...KEYWORDS)),
    mailboxIds: fc.uniqueArray(fc.constantFrom(...MAILBOXES)),
  })
  .map((held): MemberState => ({
    keywords: setOf(held.keywords),
    mailboxIds: setOf(held.mailboxIds),
  }));

const path = fc.oneof(
  fc.constantFrom(...KEYWORDS).map((keyword) => pathOf("keywords", keyword)),
  fc.constantFrom(...MAILBOXES).map((id) => pathOf("mailboxIds", id)),
);

const patch = fc
  .uniqueArray(fc.tuple(path, fc.constantFrom(true, null)), {
    minLength: 1,
    selector: ([named]) => named,
  })
  .map((entries): EmailPatch => Object.fromEntries(entries));

// A pending row: a patch, or now and then a destroy.
const row = fc.tuple(patch, fc.boolean()).map(([one, destroy]) => ({ patch: one, destroy }));

test("a patch sets a keyword or a mailbox with true and takes it off with null; its inverse says what stood there", () => {
  const read = patched(
    { keywords: { $flagged: true }, mailboxIds: { inbox: true } },
    {
      "keywords/$seen": true,
      "keywords/$flagged": null,
      "mailboxIds/inbox": null,
      "mailboxIds/archive": true,
    },
  );
  expect(read.state).toEqual({ keywords: { $seen: true }, mailboxIds: { archive: true } });
  expect(read.inverse).toEqual({
    "keywords/$seen": null,
    "keywords/$flagged": true,
    "mailboxIds/inbox": true,
    "mailboxIds/archive": null,
  });
});

test("a path names its keyword or mailbox with the escapes of a pointer token (RFC 6901 section 4)", () => {
  const none = { keywords: {}, mailboxIds: {} };
  expect(
    patched(none, { "keywords/a~1b": true, "keywords/c~0d": true, "mailboxIds/x~1y": true }).state,
  ).toEqual({ keywords: { "a/b": true, "c~d": true }, mailboxIds: { "x/y": true } });
  expect(pathOf("mailboxIds", "~1")).toBe("mailboxIds/~01");
  expect(patched(none, { "mailboxIds/~01": true }).state.mailboxIds).toEqual({ "~1": true });
});

test("a path that names neither a keyword nor a mailbox is refused", () => {
  const none = { keywords: {}, mailboxIds: {} };
  expect(() => patched(none, { "keywords/": true })).toThrow("keywords or mailboxes alone");
  expect(() => patched(none, { "mailboxIds/": true })).toThrow("keywords or mailboxes alone");
  expect(() => patched(none, { keywords: true })).toThrow("keywords or mailboxes alone");
  expect(() => patched(none, { "threadId/t1": true })).toThrow("keywords or mailboxes alone");
});

test("a patch alters the state only when some path says something new", () => {
  expect(alters({ "keywords/$seen": true }, { "keywords/$seen": null })).toBe(true);
  expect(alters({ "mailboxIds/inbox": null }, { "mailboxIds/inbox": null })).toBe(false);
});

test("a destroy in the log empties the mailboxes the email stands in when it is laid, and its inverse puts them back", () => {
  const held: MemberState = { keywords: { $seen: true }, mailboxIds: { trash: true, label: true } };
  const replay = replayed(held, [rowOf(1, {}, true)]);
  expect(replay.state).toEqual({ keywords: { $seen: true }, mailboxIds: {} });
  expect(replay.rows[0]?.inverse).toEqual({ "mailboxIds/trash": true, "mailboxIds/label": true });
  expect(unpatched(replay.state, replay.rows)).toEqual(held);
  expect(emptied({ keywords: {}, mailboxIds: { "x/y": true } })).toEqual({
    "mailboxIds/x~1y": null,
  });
});

test("a patch and its inverse compose to the state they started on", () => {
  fc.assert(
    fc.property(state, patch, (held, one) => {
      const step = patched(held, one);
      expect(patched(step.state, step.inverse).state).toEqual(held);
    }),
  );
});

test("taking the pending rows back from what they made restores the state they started on, destroys included", () => {
  fc.assert(
    fc.property(state, fc.array(row, { maxLength: 4 }), (held, rows) => {
      const log = rows.map((one, index) => rowOf(index + 1, one.patch, one.destroy));
      const replay = replayed(held, log);
      expect(unpatched(replay.state, replay.rows)).toEqual(held);
    }),
  );
});

test("a patch laid twice is the patch laid once", () => {
  fc.assert(
    fc.property(state, patch, (held, one) => {
      const once = patched(held, one).state;
      expect(patched(once, one).state).toEqual(once);
    }),
  );
});
