// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";
import { expect, test } from "vitest";

import { MAILBOXES } from "./fixtures";
import { buildTree, countOf, flatten, landingMailbox } from "./tree";
import type { TreeRow } from "./tree";

function named(id: string): Mailbox {
  const mailbox = MAILBOXES.find((row) => row.id === id);
  if (mailbox === undefined) {
    throw new Error(`no fixture named ${id}`);
  }
  return mailbox;
}

function folder(id: string, name: string, parentId: string | null = null): Mailbox {
  return { ...named("mb-facturen"), id, name, parentId };
}

function names(rows: readonly TreeRow[]): string[] {
  return rows.map((row) => row.mailbox.name);
}

test("the roles come first in their fixed order, the rest under folders by sort order then name", () => {
  const model = buildTree(MAILBOXES, "en");
  expect(names(model.roles)).toEqual(["Inbox", "Drafts", "Sent", "Archive", "Junk", "Trash"]);
  expect(names(model.folders)).toEqual(["Facturen", "Verbouwing"]);
});

test("a folder nests under its parent; one under a role or a missing parent starts at the top", () => {
  const model = buildTree(
    [
      named("mb-inbox"),
      named("mb-verbouwing"),
      named("mb-offertes"),
      folder("mb-bonnen", "Bonnen", "mb-gone"),
      folder("mb-nieuws", "Nieuwsbrieven", "mb-inbox"),
    ],
    "en",
  );
  expect(names(model.folders)).toEqual(["Bonnen", "Nieuwsbrieven", "Verbouwing"]);
  expect(names(model.folders[2]?.children ?? [])).toEqual(["Offertes"]);
});

test("the roles keep their letters and a folder takes the first free letter of its name", () => {
  const model = buildTree(
    [
      named("mb-inbox"),
      named("mb-trash"),
      folder("mb-archief", "Archief"),
      named("mb-facturen"),
      { ...folder("mb-fotos", "Foto's"), sortOrder: 11 },
      { ...folder("mb-2024", "2024"), sortOrder: 12 },
    ],
    "en",
  );
  expect(model.roles.map((row) => row.letter)).toEqual(["i", "t"]);
  expect(model.folders.map((row) => [row.mailbox.name, row.letter])).toEqual([
    ["Archief", "r"],
    ["Facturen", "f"],
    ["Foto's", "o"],
    ["2024", null],
  ]);
});

test("drafts count their drafts and every other mailbox its unread mail", () => {
  expect(countOf(named("mb-inbox"))).toBe(23);
  expect(countOf(named("mb-drafts"))).toBe(2);
  expect(countOf(named("mb-sent"))).toBe(0);
});

test("the landing mailbox is the inbox, else the first row drawn, else none", () => {
  expect(landingMailbox(buildTree(MAILBOXES, "en"))?.id).toBe("mb-inbox");
  expect(landingMailbox(buildTree([named("mb-facturen"), named("mb-trash")], "en"))?.id).toBe(
    "mb-trash",
  );
  expect(landingMailbox(buildTree([named("mb-facturen")], "en"))?.id).toBe("mb-facturen");
  expect(landingMailbox(buildTree([], "en"))).toBeNull();
});

test("a flat tree numbers every row among its siblings and by depth", () => {
  const model = buildTree(MAILBOXES, "en");
  expect(
    flatten(model.folders).map((row) => [row.mailbox.name, row.level, row.position, row.size]),
  ).toEqual([
    ["Facturen", 1, 1, 2],
    ["Verbouwing", 1, 2, 2],
    ["Offertes", 2, 1, 1],
  ]);
});
