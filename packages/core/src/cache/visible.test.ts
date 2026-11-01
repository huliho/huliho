// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import type { Mailbox } from "../jmap/schemas";
import { mailbox } from "./fake-jmap";
import { visibleMailboxes } from "./visible";

function box(
  id: string,
  facts: { role?: string; parentId?: string; subscribed?: boolean } = {},
): Mailbox {
  return {
    ...mailbox(id, facts.role ?? null),
    parentId: facts.parentId ?? null,
    isSubscribed: facts.subscribed ?? true,
  };
}

function ids(mailboxes: readonly Mailbox[]): string[] {
  return visibleMailboxes(mailboxes).map(({ id }) => id);
}

test("the subscribed mailboxes and the roles are drawn, a hidden folder is not", () => {
  expect(
    ids([
      box("inbox", { role: "inbox", subscribed: false }),
      box("trash", { role: "trash" }),
      box("work"),
      box("old", { subscribed: false }),
    ]),
  ).toEqual(["inbox", "trash", "work"]);
});

test("an account that subscribes nothing has every mailbox drawn (RFC 8621 section 2)", () => {
  const unsubscribed = [
    box("inbox", { role: "inbox", subscribed: false }),
    box("junk", { role: "junk", subscribed: false }),
    box("old", { subscribed: false }),
  ];
  expect(ids(unsubscribed)).toEqual(["inbox", "junk", "old"]);
  expect(ids([])).toEqual([]);
});

test("a hidden folder above a subscribed folder is drawn so the folder keeps its place (RFC 5258 section 3.5)", () => {
  expect(
    ids([
      box("inbox", { role: "inbox" }),
      box("projects", { subscribed: false }),
      box("clients", { parentId: "projects", subscribed: false }),
      box("acme", { parentId: "clients" }),
      box("archive", { subscribed: false }),
    ]),
  ).toEqual(["inbox", "projects", "clients", "acme"]);
});

test("a role draws no folder above it, and parents that name each other end the walk", () => {
  expect(
    ids([
      box("gmail", { subscribed: false }),
      box("sent", { role: "sent", parentId: "gmail" }),
      box("a", { parentId: "b", subscribed: false }),
      box("b", { parentId: "a" }),
    ]),
  ).toEqual(["sent", "a", "b"]);
});
