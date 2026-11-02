// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import type { Mailbox } from "../jmap/schemas";
import { mailbox } from "./fake-jmap";
import { branchOf, hideWrite, visibleMailboxes } from "./visible";

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

test("a branch holds the folder and every folder without a role under it", () => {
  const rows = [
    box("parent"),
    box("child", { parentId: "parent", subscribed: false }),
    box("deep", { parentId: "child" }),
    box("role", { parentId: "parent", role: "junk" }),
    box("under-role", { parentId: "role" }),
    box("other"),
  ];
  expect(branchOf(rows, "parent")).toEqual(["parent", "child", "deep"]);
  expect(branchOf(rows, "other")).toEqual(["other"]);
});

test("a loop in the server's parents ends the branch", () => {
  const rows = [box("a", { parentId: "b" }), box("b", { parentId: "a" })];
  expect(branchOf(rows, "a")).toEqual(["a", "b"]);
});

test("a hide unsubscribes the subscribed folders of the branch, never a role", () => {
  const rows = [
    box("parent", { subscribed: false }),
    box("child", { parentId: "parent" }),
    box("quiet", { parentId: "parent", subscribed: false }),
    box("role", { parentId: "parent", role: "junk" }),
    box("other"),
  ];
  expect(hideWrite(rows, "parent")).toEqual({ update: { child: { isSubscribed: false } } });
});
