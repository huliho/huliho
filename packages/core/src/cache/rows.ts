// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailHeader, Mailbox } from "../jmap/schemas";
import type { MemberState, ThreadRow } from "./store";
import { SEEN } from "./counts";
import type { WindowPage } from "./window";

// One row of a list: the thread as this mailbox shows it, read from the
// exemplar's header and the state of every member in the mailbox.
export interface ListRow {
  // The exemplar's email id, which names the row.
  id: string;
  threadId: string;
  sender: string | null;
  subject: string | null;
  preview: string;
  receivedAt: string;
  unread: boolean;
  flagged: boolean;
  hasAttachment: boolean;
  // The thread's emails in this mailbox, oldest first, which an action
  // on the row takes.
  memberIds: string[];
  count: number;
}

// The progress of a mailbox's first header sync, null once it is done
// or on an account whose server holds no such state.
export interface FirstSync {
  synced: number;
  total: number;
}

const FLAGGED = "$flagged";

function senderOf(email: EmailHeader): string | null {
  const address = email.from?.[0] ?? email.sender?.[0];
  if (address === undefined) {
    return null;
  }
  return address.name === null || address.name === "" ? address.email : address.name;
}

// The members that stand in the mailbox in the thread's order, oldest
// first; the exemplar alone when the thread row names none there and the
// exemplar does.
function membersIn(
  email: EmailHeader,
  thread: ThreadRow | undefined,
  mailboxId: string,
): [string, MemberState][] {
  const states = new Map(Object.entries(thread?.members ?? {}));
  const inside = (thread?.emailIds ?? []).flatMap((id): [string, MemberState][] => {
    const state = states.get(id);
    return state !== undefined && mailboxId in state.mailboxIds ? [[id, state]] : [];
  });
  if (inside.length > 0) {
    return inside;
  }
  return mailboxId in email.mailboxIds ? [[email.id, email]] : [];
}

// The rows of one page, in the server's order; an id without a header
// is left out, and so is a thread with no member left in the mailbox.
export function listRows(page: WindowPage, mailboxId: string): ListRow[] {
  const emails = new Map(Object.entries(page.emails));
  const threads = new Map(Object.entries(page.threads));
  return page.ids.flatMap((id) => {
    const email = emails.get(id);
    if (email === undefined) {
      return [];
    }
    const members = membersIn(email, threads.get(email.threadId), mailboxId);
    if (members.length === 0) {
      return [];
    }
    return [
      {
        id,
        threadId: email.threadId,
        sender: senderOf(email),
        subject: email.subject,
        preview: email.preview,
        receivedAt: email.receivedAt,
        unread: members.some(([, member]) => !(SEEN in member.keywords)),
        flagged: members.some(([, member]) => FLAGGED in member.keywords),
        hasAttachment: email.hasAttachment,
        memberIds: members.map(([memberId]) => memberId),
        count: members.length,
      },
    ];
  });
}

// One page of a list as the client renders it: the rows, the frozen
// total and the marker's count.
export interface ListPage {
  rows: ListRow[];
  total: number | null;
  pending: number;
}

export function listPage(page: WindowPage, mailboxId: string): ListPage {
  return { rows: listRows(page, mailboxId), total: page.total, pending: page.pending };
}

// A bridge mailbox still fetching its headers: fewer synced than the
// server holds.
export function firstSyncOf(mailbox: Mailbox): FirstSync | null {
  if (mailbox.syncedEmails === undefined || mailbox.syncedEmails >= mailbox.totalEmails) {
    return null;
  }
  return { synced: mailbox.syncedEmails, total: mailbox.totalEmails };
}
