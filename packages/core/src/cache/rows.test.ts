// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { at, email, mailbox } from "./fake-jmap";
import { actingMailbox, firstSyncOf, listRows, membersInScope, resultRows } from "./rows";
import type { Scope } from "./scope";
import type { MemberState, ThreadRow } from "./store";
import { treePlaces } from "./tree-order";
import type { WindowPage } from "./window";

const INBOX = "inbox";

const TREE = treePlaces(
  [
    mailbox(INBOX, "inbox"),
    mailbox("archive", "archive"),
    mailbox("junk", "junk"),
    mailbox("trash", "trash"),
    { ...mailbox("work"), name: "Work" },
    { ...mailbox("alpha"), name: "Alpha" },
  ],
  "en",
);

const DEFAULT: Scope = { kind: "except", mailboxIds: ["junk", "trash"] };

function state(...mailboxIds: string[]): MemberState {
  return {
    keywords: {},
    mailboxIds: Object.fromEntries(mailboxIds.map((id) => [id, true as const])),
  };
}

function thread(id: string, members: ThreadRow["members"]): ThreadRow {
  return { id, emailIds: Object.keys(members), members };
}

function page(rows: Partial<WindowPage>): WindowPage {
  return { ids: [], total: null, pending: 0, emails: {}, threads: {}, ...rows };
}

test("a row reads its state from the members in the mailbox alone", () => {
  const exemplar = email("e3", { threadId: "t1", keywords: ["$seen"], receivedAt: at(3) });
  const rows = listRows(
    page({
      ids: ["e3"],
      emails: { e3: exemplar },
      threads: {
        t1: thread("t1", {
          e1: { keywords: { $seen: true }, mailboxIds: { archive: true } },
          e2: { keywords: {}, mailboxIds: { [INBOX]: true } },
          e3: { keywords: { $seen: true, $flagged: true }, mailboxIds: { [INBOX]: true } },
        }),
      },
    }),
    INBOX,
  );
  expect(rows).toEqual([
    {
      id: "e3",
      threadId: "t1",
      mailboxId: INBOX,
      sender: "Sanne",
      subject: "Message e3",
      preview: "Body of e3.",
      receivedAt: at(3),
      unread: true,
      flagged: true,
      hasAttachment: false,
      memberIds: ["e2", "e3"],
      count: 2,
    },
  ]);
});

test("a thread the store lacks counts the exemplar alone and an unread member elsewhere does not count", () => {
  const exemplar = email("e1", { threadId: "t1", keywords: ["$seen"], receivedAt: at(1) });
  const alone = listRows(page({ ids: ["e1"], emails: { e1: exemplar } }), INBOX);
  expect(alone[0]).toMatchObject({ unread: false, flagged: false, count: 1 });
  const elsewhere = listRows(
    page({
      ids: ["e1"],
      emails: { e1: exemplar },
      threads: {
        t1: thread("t1", {
          e1: { keywords: { $seen: true }, mailboxIds: { [INBOX]: true } },
          e9: { keywords: {}, mailboxIds: { trash: true } },
        }),
      },
    }),
    INBOX,
  );
  expect(elsewhere[0]).toMatchObject({ unread: false, count: 1 });
});

test("a thread with no member left in the mailbox leaves the page; one the row lacks states for reads its exemplar", () => {
  const moved = email("e1", { threadId: "t1", mailboxIds: ["archive"], receivedAt: at(1) });
  const gone = email("e2", { threadId: "t2", mailboxIds: [], receivedAt: at(2) });
  const unknown = email("e3", { threadId: "t3", receivedAt: at(3) });
  const rows = listRows(
    page({
      ids: ["e3", "e2", "e1"],
      emails: { e1: moved, e2: gone, e3: unknown },
      threads: {
        t1: thread("t1", { e1: { keywords: {}, mailboxIds: { archive: true } } }),
        t3: { id: "t3", emailIds: ["e3"], members: {} },
      },
    }),
    INBOX,
  );
  expect(rows.map((row) => [row.id, row.count])).toEqual([["e3", 1]]);
});

test("the sender is the display name, else the address, else nothing; a missing header leaves the row out", () => {
  const named = email("e1", { receivedAt: at(1) });
  const bare = {
    ...email("e2", { receivedAt: at(2) }),
    from: [{ name: "", email: "mo@example.test" }],
  };
  const nobody = { ...email("e3", { receivedAt: at(3) }), from: null, sender: null };
  const rows = listRows(
    page({ ids: ["e1", "e2", "e3", "e4"], emails: { e1: named, e2: bare, e3: nobody } }),
    INBOX,
  );
  expect(rows.map((row) => row.sender)).toEqual(["Sanne", "mo@example.test", null]);
});

test("a first sync stands while fewer emails are synced than the server holds", () => {
  const row = { ...mailbox("inbox", "inbox"), totalEmails: 10, syncedEmails: 4 };
  expect(firstSyncOf(row)).toEqual({ synced: 4, total: 10 });
  expect(firstSyncOf({ ...row, syncedEmails: 10 })).toBeNull();
  const { syncedEmails, ...native } = row;
  expect(syncedEmails).toBe(4);
  expect(firstSyncOf(native)).toBeNull();
});

test("the members a scope covers stand in the thread's order under each kind of scope", () => {
  const exemplar = email("e3", { threadId: "t1", receivedAt: at(3) });
  const row = thread("t1", { e1: state("archive"), e2: state("trash"), e3: state(INBOX, "work") });
  const ids = (scope: Scope): string[] => membersInScope(exemplar, row, scope).map(([id]) => id);
  expect(ids({ kind: "one", mailboxId: INBOX })).toEqual(["e3"]);
  expect(ids(DEFAULT)).toEqual(["e1", "e3"]);
  expect(ids({ kind: "all" })).toEqual(["e1", "e2", "e3"]);
  expect(ids({ kind: "one", mailboxId: "junk" })).toEqual([]);
});

test("a result acts from the scope's mailbox, else the inbox, else the first mailbox in the tree's order", () => {
  const work: Scope = { kind: "one", mailboxId: "work" };
  expect(actingMailbox(state("work", INBOX), [], work, TREE)).toBe("work");
  expect(actingMailbox(state("work", INBOX), [], DEFAULT, TREE)).toBe(INBOX);
  // The archive is a role, so the tree draws it above every folder.
  expect(actingMailbox(state("work", "archive"), [], DEFAULT, TREE)).toBe("archive");
  expect(actingMailbox(state("work", "alpha"), [], DEFAULT, TREE)).toBe("alpha");
  // Trash stands outside the default scope; under all it counts.
  expect(actingMailbox(state("trash", "work"), [], DEFAULT, TREE)).toBe("work");
  expect(actingMailbox(state("trash"), [], { kind: "all" }, TREE)).toBe("trash");
});

test("an exemplar that left the scope gives way to the members still in it", () => {
  const members = [state("work"), state("archive")];
  expect(actingMailbox(state("trash"), members, DEFAULT, TREE)).toBe("archive");
  expect(actingMailbox(state(), [], DEFAULT, TREE)).toBeNull();
});

test("a result reads and acts as its mailbox shows the thread, and a thread with no member in scope shows none", () => {
  const exemplar = email("e3", { threadId: "t1", keywords: ["$seen"], receivedAt: at(3) });
  const binned = email("e4", { threadId: "t2", mailboxIds: ["trash"], receivedAt: at(4) });
  const rows = resultRows(
    page({
      ids: ["e4", "e3"],
      emails: { e3: exemplar, e4: binned },
      threads: {
        t1: thread("t1", {
          e1: { keywords: {}, mailboxIds: { archive: true } },
          e2: { keywords: { $seen: true }, mailboxIds: { [INBOX]: true } },
          e3: { keywords: { $seen: true }, mailboxIds: { [INBOX]: true } },
        }),
        t2: thread("t2", { e4: { keywords: {}, mailboxIds: { trash: true } } }),
      },
    }),
    DEFAULT,
    TREE,
  );
  expect(rows).toEqual([
    expect.objectContaining({
      id: "e3",
      mailboxId: INBOX,
      unread: false,
      memberIds: ["e2", "e3"],
      count: 2,
    }),
  ]);
});
