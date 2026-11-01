// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";
import { expect, test } from "vitest";

import { fixtureCache } from "../fixture-cache";
import { FOLDER_LIMITS } from "../fixtures";
import { folderItems, withFolderItems } from "./folder-items";
import type { FolderItem } from "./folder-jobs";
import { MANAGED, managedOf } from "./writable-rig";

function nothing(): void {
  // An item drawn here runs nothing.
}

function labelsOf(mailbox: Mailbox, mailboxes: readonly Mailbox[] = MANAGED): string[] {
  return folderItems({
    locale: "en",
    cache: fixtureCache({}),
    accountId: "acc-1",
    mailbox,
    mailboxes,
    limits: FOLDER_LIMITS,
    shownId: undefined,
    leave: nothing,
  }).map((item) => item.label);
}

function drawn(id: FolderItem["id"], danger: boolean): FolderItem {
  return { id, label: id, danger, run: nothing };
}

test("INBOX takes a subfolder alone, a role mailbox a new name too, a folder every item", () => {
  expect(labelsOf(managedOf("mb-inbox"))).toEqual(["New subfolder"]);
  expect(labelsOf(managedOf("mb-sent"))).toEqual(["New subfolder", "Rename"]);
  expect(labelsOf(managedOf("mb-facturen"))).toEqual(["New subfolder", "Rename", "Hide", "Delete"]);
});

test("each item but Hide needs its right; INBOX stays without a new name whatever it holds", () => {
  const facturen = managedOf("mb-facturen");
  const bare = {
    ...facturen,
    myRights: { ...facturen.myRights, mayCreateChild: false, mayRename: false, mayDelete: false },
  };
  expect(labelsOf(bare)).toEqual(["Hide"]);
  const inbox = managedOf("mb-inbox");
  expect(labelsOf({ ...inbox, myRights: { ...inbox.myRights, mayRename: true } })).toEqual([
    "New subfolder",
  ]);
  const sent = managedOf("mb-sent");
  expect(labelsOf({ ...sent, myRights: { ...sent.myRights, mayDelete: true } })).toEqual([
    "New subfolder",
    "Rename",
  ]);
});

test("an account that keeps no subscriptions offers no Hide, since the tree draws every mailbox", () => {
  const rows = MANAGED.map((row) => ({ ...row, isSubscribed: false }));
  const facturen = rows.find((row) => row.id === "mb-facturen") ?? managedOf("mb-facturen");
  expect(labelsOf(facturen, rows)).toEqual(["New subfolder", "Rename", "Delete"]);
});

// The managed tree with `box` in the place of the mailbox of its id.
function managedWith(box: Mailbox): Mailbox[] {
  return MANAGED.map((row) => (row.id === box.id ? box : row));
}

test("a folder the tree does not draw offers no Hide; one drawn only above a subscribed folder does", () => {
  const hidden = { ...managedOf("mb-facturen"), isSubscribed: false };
  expect(labelsOf(hidden, managedWith(hidden))).toEqual(["New subfolder", "Rename", "Delete"]);
  const parent = { ...managedOf("mb-verbouwing"), isSubscribed: false };
  expect(labelsOf(parent, managedWith(parent))).toEqual([
    "New subfolder",
    "Rename",
    "Hide",
    "Delete",
  ]);
});

test("the folder items stand after Mark all as read and before the items that empty", () => {
  const ordered = withFolderItems(
    [drawn("markRead", false), drawn("empty", true)],
    [drawn("rename", false), drawn("delete", true)],
  );
  expect(ordered.map((entry) => entry.id)).toEqual(["markRead", "rename", "delete", "empty"]);
  const facturen = folderItems({
    locale: "en",
    cache: fixtureCache({}),
    accountId: "acc-1",
    mailbox: managedOf("mb-facturen"),
    mailboxes: MANAGED,
    limits: FOLDER_LIMITS,
    shownId: undefined,
    leave: nothing,
  });
  expect(facturen.filter((entry) => entry.danger).map((entry) => entry.id)).toEqual(["delete"]);
});
