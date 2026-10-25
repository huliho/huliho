// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import type { Mailbox } from "../jmap/schemas";
import { mailbox } from "./fake-jmap";
import { treeOrder, treePlaces } from "./tree-order";

function folder(id: string, name: string, facts: Partial<Mailbox> = {}): Mailbox {
  return { ...mailbox(id), name, ...facts };
}

test("the roles stand first in their own order, then the folders with each one's children right after it", () => {
  const mailboxes = [
    folder("b", "beta"),
    { ...mailbox("trash", "trash"), name: "Trash" },
    folder("a2", "child", { parentId: "a" }),
    folder("a", "Alpha"),
    { ...mailbox("inbox", "inbox"), name: "Inbox" },
    folder("in-inbox", "Under the inbox", { parentId: "inbox" }),
    { ...mailbox("sent", "sent"), name: "Sent" },
  ];
  expect(treeOrder(mailboxes, "en")).toEqual([
    "inbox",
    "sent",
    "trash",
    "a",
    "a2",
    "b",
    "in-inbox",
  ]);
});

test("siblings sort by sortOrder, then by name as the locale sorts it, any case", () => {
  const mailboxes = [
    folder("z", "zeta", { sortOrder: 1 }),
    folder("e", "Éclair"),
    folder("d", "delta"),
    folder("b", "Beta"),
  ];
  expect(treeOrder(mailboxes, "en")).toEqual(["z", "b", "d", "e"]);
});

test("a second mailbox of a role stands among the folders, and a cycle of parents stands nowhere", () => {
  const mailboxes = [
    { ...mailbox("inbox-2", "inbox"), name: "Old inbox", sortOrder: 20 },
    { ...mailbox("inbox", "inbox"), name: "Inbox" },
    folder("x", "x", { parentId: "y" }),
    folder("y", "y", { parentId: "x" }),
  ];
  expect(treeOrder(mailboxes, "en")).toEqual(["inbox", "inbox-2"]);
  expect(treePlaces(mailboxes, "en")).toEqual(
    new Map([
      ["inbox", 0],
      ["inbox-2", 1],
    ]),
  );
});
