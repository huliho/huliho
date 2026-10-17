// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { actionFor, actionMutations, offeredActions, verbOf } from "./actions";
import type { ActionTarget } from "./actions";
import { mailbox } from "./cache/fake-jmap";
import type { Mutation } from "./cache/pending";
import type { Mailbox } from "./jmap/schemas";

type Rights = Mailbox["myRights"];

// The patch of the first change, where it is one.
function firstPatch(mutations: Mutation[]): unknown {
  const first = mutations[0];
  return first !== undefined && "patch" in first ? first.patch : undefined;
}

function moved(from: string, to: string): Record<string, true | null> {
  return { [`mailboxIds/${from}`]: null, [`mailboxIds/${to}`]: true };
}

function box(id: string, role: string | null, rights: Partial<Rights> = {}): Mailbox {
  const row = mailbox(id, role);
  return {
    ...row,
    myRights: { ...row.myRights, mayAddItems: true, mayRemoveItems: true, ...rights },
  };
}

const INBOX = box("inbox", "inbox");
const ARCHIVE = box("archive", "archive");
const TRASH = box("trash", "trash");
const JUNK = box("junk", "junk");
const FOLDER = box("folder", null);
const TREE = [INBOX, ARCHIVE, TRASH, JUNK, FOLDER];

const THREAD: ActionTarget = {
  threadId: "t1",
  memberIds: ["e1", "e2"],
  unread: true,
  flagged: false,
};

test("a mailbox without mayRemoveItems offers no move, whatever the others allow", () => {
  const locked = box("inbox", "inbox", { mayRemoveItems: false });
  expect(offeredActions(locked, [locked, ARCHIVE, TRASH, JUNK])).toMatchObject({
    archive: null,
    delete: null,
    spam: null,
  });
  const trash = box("trash", "trash", { mayRemoveItems: false });
  expect(offeredActions(trash, [INBOX, trash])).toMatchObject({
    delete: null,
    deleteForever: false,
  });
});

test("a target without mayAddItems or without its role is no target", () => {
  const sealed = [INBOX, box("archive", "archive", { mayAddItems: false }), JUNK];
  expect(offeredActions(INBOX, sealed)).toMatchObject({
    archive: null,
    delete: null,
    spam: "junk",
  });
  expect(offeredActions(JUNK, [JUNK]).spam).toBeNull();
});

test("the marks follow maySetSeen and maySetKeywords of the mailbox shown", () => {
  const plain = box("inbox", "inbox", { maySetSeen: false, maySetKeywords: false });
  expect(offeredActions(plain, TREE)).toMatchObject({
    seen: false,
    flag: false,
    junkKeywords: false,
  });
  expect(actionMutations("read", plain, offeredActions(plain, TREE), THREAD)).toEqual([]);
  expect(actionMutations("star", plain, offeredActions(plain, TREE), THREAD)).toEqual([]);
});

test("each role offers its own set", () => {
  expect(offeredActions(INBOX, TREE)).toMatchObject({
    archive: "archive",
    delete: "trash",
    deleteForever: false,
    spam: "junk",
  });
  expect(offeredActions(ARCHIVE, TREE)).toMatchObject({
    archive: null,
    delete: "trash",
    deleteForever: false,
    spam: "junk",
  });
  expect(offeredActions(TRASH, TREE)).toMatchObject({
    archive: null,
    delete: null,
    deleteForever: true,
    spam: "junk",
  });
  expect(offeredActions(JUNK, TREE)).toMatchObject({
    archive: null,
    delete: null,
    deleteForever: true,
    spam: "inbox",
  });
  expect(offeredActions(FOLDER, [FOLDER, INBOX])).toMatchObject({
    archive: null,
    delete: null,
    deleteForever: false,
    spam: null,
  });
});

test("a key names delete forever and not spam where the mailbox asks, the flips by the thread", () => {
  expect(actionFor("delete", TRASH, THREAD)).toBe("deleteForever");
  expect(actionFor("delete", JUNK, THREAD)).toBe("deleteForever");
  expect(actionFor("delete", INBOX, THREAD)).toBe("delete");
  expect(actionFor("spam", JUNK, THREAD)).toBe("notSpam");
  expect(actionFor("spam", TRASH, THREAD)).toBe("spam");
  expect(actionFor("read", INBOX, THREAD)).toBe("read");
  expect(actionFor("read", INBOX, { ...THREAD, unread: false })).toBe("unread");
  expect(actionFor("star", INBOX, THREAD)).toBe("star");
  expect(actionFor("star", INBOX, { ...THREAD, flagged: true })).toBe("unstar");
  expect(actionFor("archive", INBOX, THREAD)).toBe("archive");
});

test("a move takes every email of the thread in the mailbox shown, the removal and the addition in one patch", () => {
  const offered = offeredActions(INBOX, TREE);
  expect(actionMutations("archive", INBOX, offered, THREAD)).toEqual(
    ["e1", "e2"].map((id) => ({
      type: "Email",
      id,
      threadId: "t1",
      patch: { "mailboxIds/inbox": null, "mailboxIds/archive": true },
    })),
  );
  expect(firstPatch(actionMutations("delete", INBOX, offered, THREAD))).toEqual(
    moved("inbox", "trash"),
  );
});

test("spam writes $junk beside the move and not spam $notjunk; without maySetKeywords the move alone", () => {
  expect(firstPatch(actionMutations("spam", INBOX, offeredActions(INBOX, TREE), THREAD))).toEqual({
    ...moved("inbox", "junk"),
    "keywords/$junk": true,
    "keywords/$notjunk": null,
  });
  expect(firstPatch(actionMutations("notSpam", JUNK, offeredActions(JUNK, TREE), THREAD))).toEqual({
    ...moved("junk", "inbox"),
    "keywords/$junk": null,
    "keywords/$notjunk": true,
  });
  const plain = box("inbox", "inbox", { maySetKeywords: false });
  expect(firstPatch(actionMutations("spam", plain, offeredActions(plain, TREE), THREAD))).toEqual(
    moved("inbox", "junk"),
  );
  expect(actionMutations("spam", JUNK, offeredActions(JUNK, TREE), THREAD)).toEqual([]);
  expect(actionMutations("notSpam", INBOX, offeredActions(INBOX, TREE), THREAD)).toEqual([]);
});

test("delete forever destroys every email in Trash and Junk alone; delete stands elsewhere", () => {
  expect(actionMutations("deleteForever", TRASH, offeredActions(TRASH, TREE), THREAD)).toEqual(
    ["e1", "e2"].map((id) => ({ type: "Email", id, threadId: "t1", destroy: true })),
  );
  expect(actionMutations("deleteForever", INBOX, offeredActions(INBOX, TREE), THREAD)).toEqual([]);
  expect(actionMutations("delete", TRASH, offeredActions(TRASH, TREE), THREAD)).toEqual([]);
});

test("read marks every email, unread the newest; star flags the newest and unstar clears each", () => {
  const offered = offeredActions(INBOX, TREE);
  const ids = (action: Parameters<typeof actionMutations>[0]): string[] =>
    actionMutations(action, INBOX, offered, THREAD).map(({ id }) => id);
  expect(ids("read")).toEqual(["e1", "e2"]);
  expect(ids("unread")).toEqual(["e2"]);
  expect(ids("star")).toEqual(["e2"]);
  expect(ids("unstar")).toEqual(["e1", "e2"]);
  expect(actionMutations("unread", INBOX, offered, { ...THREAD, memberIds: [] })).toEqual([]);
});

test("a refused change reads by what it did", () => {
  expect(verbOf({ patch: {}, destroy: true }, TREE)).toBe("deleteForever");
  expect(verbOf({ patch: moved("inbox", "archive") }, TREE)).toBe("archive");
  expect(verbOf({ patch: moved("inbox", "trash") }, TREE)).toBe("delete");
  expect(verbOf({ patch: moved("inbox", "junk") }, TREE)).toBe("spam");
  expect(verbOf({ patch: moved("junk", "inbox") }, TREE)).toBe("notSpam");
  expect(verbOf({ patch: moved("archive", "inbox") }, TREE)).toBe("move");
  expect(verbOf({ patch: moved("inbox", "folder") }, TREE)).toBe("move");
  expect(verbOf({ patch: { "keywords/$seen": true } }, TREE)).toBe("read");
  expect(verbOf({ patch: { "keywords/$seen": null } }, TREE)).toBe("unread");
  expect(verbOf({ patch: { "keywords/$flagged": true } }, TREE)).toBe("star");
  expect(verbOf({ patch: { "keywords/$flagged": null } }, TREE)).toBe("unstar");
  expect(verbOf({ patch: { "keywords/$junk": true } }, TREE)).toBe("move");
});
