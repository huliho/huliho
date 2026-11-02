// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";
import { expect, test } from "vitest";

import { closedFolders, hiddenFolders, roleRows, roleWrite, showWrite } from "./folder-choices";
import { FOLDERS, closedBox, fixedBox, folderBox, hiddenBox, roleBox } from "./folder-fixtures";

function rowOf(mailboxes: readonly Mailbox[], role: string) {
  const row = roleRows(mailboxes, "en").find((entry) => entry.role === role);
  if (row === undefined) {
    throw new Error(`no row for ${role}`);
  }
  return row;
}

function ids(mailboxes: readonly Mailbox[], role: string): string[] {
  return rowOf(mailboxes, role).choices.map(({ id }) => id);
}

test("the six rows stand in the tree's order, each prefilled from the role the server answers", () => {
  const rows = roleRows(FOLDERS, "en");
  expect(rows.map(({ role }) => role)).toEqual([
    "inbox",
    "drafts",
    "sent",
    "archive",
    "junk",
    "trash",
  ]);
  expect(rows.map(({ holderId }) => holderId)).toEqual([
    "mb-inbox",
    "mb-drafts",
    "mb-sent",
    "mb-archive",
    null,
    "mb-trash",
  ]);
});

test("a role no mailbox holds has no holder and its row is not fixed", () => {
  expect(rowOf(FOLDERS, "junk")).toMatchObject({ holderId: null, fixed: false });
});

test("a select offers what the tree draws in its order, a nested folder by its path", () => {
  const choices = rowOf(FOLDERS, "archive").choices;
  expect(choices.map(({ label }) => label)).toEqual([
    "Drafts",
    "Sent",
    "Archive",
    "Trash",
    "Facturen",
    "Verbouwing",
    "Verbouwing / Offertes",
  ]);
});

test("a hidden folder and one the server does not open hold no role", () => {
  const mailboxes = [
    ...FOLDERS,
    hiddenBox("mb-oud", "Oud"),
    closedBox("mb-closed", "Gesloten", { subscribed: true }),
  ];
  expect(ids(mailboxes, "archive")).not.toContain("mb-oud");
  expect(ids(mailboxes, "archive")).not.toContain("mb-closed");
});

test("a mailbox the server fixes keeps its role: its row is fixed and no other row offers it", () => {
  const inbox = rowOf(FOLDERS, "inbox");
  expect(inbox.fixed).toBe(true);
  expect(inbox.choices.map(({ id }) => id)).toContain("mb-inbox");
  expect(ids(FOLDERS, "archive")).not.toContain("mb-inbox");
  const narrowed = [...FOLDERS, fixedBox("mb-spam", "Spam", "junk")];
  expect(rowOf(narrowed, "junk").fixed).toBe(true);
  expect(ids(narrowed, "archive")).not.toContain("mb-spam");
});

test("a mailbox that reads nothing takes no role, even one that takes a rename", () => {
  const closed = closedBox("mb-dicht", "Dicht", { subscribed: true });
  const renamed = { ...closed, myRights: { ...closed.myRights, mayRename: true } };
  expect(ids([...FOLDERS, renamed], "archive")).not.toContain("mb-dicht");
});

test("a folder that takes no rename takes no role, so no pick can strand one", () => {
  const locked = {
    ...folderBox("mb-locked", "Locked"),
    myRights: fixedBox("x", "x", "junk").myRights,
  };
  expect(ids([...FOLDERS, locked], "archive")).not.toContain("mb-locked");
});

test("a mailbox with a role that takes a rename goes to another row, which takes its role away", () => {
  expect(ids(FOLDERS, "archive")).toContain("mb-trash");
  expect(rowOf(FOLDERS, "trash").fixed).toBe(false);
});

test("a role move names the holder first and the taker next", () => {
  const write = roleWrite(FOLDERS, "archive", "mb-facturen");
  expect(write).toEqual({
    update: { "mb-archive": { role: null }, "mb-facturen": { role: "archive" } },
  });
  expect(Object.keys(write?.update ?? {})).toEqual(["mb-archive", "mb-facturen"]);
});

test("None takes the role away; a role without a holder sends the taker alone; the same mailbox sends nothing", () => {
  expect(roleWrite(FOLDERS, "archive", null)).toEqual({ update: { "mb-archive": { role: null } } });
  expect(roleWrite(FOLDERS, "junk", "mb-facturen")).toEqual({
    update: { "mb-facturen": { role: "junk" } },
  });
  expect(roleWrite(FOLDERS, "archive", "mb-archive")).toBeNull();
  expect(roleWrite(FOLDERS, "junk", null)).toBeNull();
});

test("the hidden list holds what the tree leaves out, by the tree's order and by path", () => {
  const mailboxes = [
    ...FOLDERS,
    hiddenBox("mb-zolder", "Zolder"),
    hiddenBox("mb-kelder", "Kelder"),
    hiddenBox("mb-oud", "Oud", "mb-kelder"),
  ];
  expect(hiddenFolders(mailboxes, "en")).toEqual([
    { id: "mb-kelder", label: "Kelder" },
    { id: "mb-oud", label: "Kelder / Oud" },
    { id: "mb-zolder", label: "Zolder" },
  ]);
  expect(hiddenFolders(FOLDERS, "en")).toEqual([]);
});

test("a folder above a subscribed folder is drawn, so the hidden list leaves it out", () => {
  const mailboxes = [
    ...FOLDERS,
    hiddenBox("mb-kelder", "Kelder"),
    folderBox("mb-oud", "Oud", "mb-kelder"),
  ];
  expect(hiddenFolders(mailboxes, "en")).toEqual([]);
});

test("an account that subscribes nothing hides nothing and offers no choice for a closed mailbox", () => {
  const nothing = [
    roleBox("mb-inbox", "Inbox", "inbox", { subscribed: false, fixed: true }),
    hiddenBox("mb-a", "A"),
    closedBox("mb-b", "B"),
  ];
  expect(hiddenFolders(nothing, "en")).toEqual([]);
  expect(closedFolders(nothing, "en")).toEqual([]);
});

test("Show subscribes the one folder; its subfolders stay as they are", () => {
  expect(showWrite("mb-kelder")).toEqual({ update: { "mb-kelder": { isSubscribed: true } } });
});

test("a closed mailbox reads as hidden where the tree leaves it out; one with a role never stands there", () => {
  const mailboxes = [
    ...FOLDERS,
    closedBox("mb-open", "Open", { subscribed: true }),
    closedBox("mb-dicht", "Dicht"),
    closedBox("mb-rol", "Rol", { subscribed: true, role: "junk" }),
  ];
  expect(closedFolders(mailboxes, "en")).toEqual([
    { id: "mb-dicht", label: "Dicht", hidden: true },
    { id: "mb-open", label: "Open", hidden: false },
  ]);
});

test("a closed mailbox above a subscribed folder is drawn, so it reads as a link", () => {
  const mailboxes = [
    ...FOLDERS,
    closedBox("mb-dicht", "Dicht"),
    folderBox("mb-oud", "Oud", "mb-dicht"),
  ];
  expect(closedFolders(mailboxes, "en")).toEqual([
    { id: "mb-dicht", label: "Dicht", hidden: false },
  ]);
});
