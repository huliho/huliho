// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { at, email, mailbox } from "./fake-jmap";
import { firstSyncOf, listRows } from "./rows";
import type { ThreadRow } from "./store";
import type { WindowPage } from "./window";

const INBOX = "inbox";

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
      sender: "Sanne",
      subject: "Message e3",
      preview: "Body of e3.",
      receivedAt: at(3),
      unread: true,
      flagged: true,
      hasAttachment: false,
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
