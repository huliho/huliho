// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailHeader, Mailbox } from "../jmap/schemas";
import type { MemberState, ThreadRow } from "./store";
import { SEEN } from "./counts";
import { memberOf } from "./members";
import { covers, inScope } from "./scope";
import type { Scope } from "./scope";
import type { TreePlaces } from "./tree-order";
import type { WindowPage } from "./window";

// One row of a list: the thread as one mailbox shows it, read from the
// exemplar's header and the state of every member in that mailbox.
export interface ListRow {
  // The exemplar's email id, which names the row.
  id: string;
  threadId: string;
  // The mailbox the row reads and acts as: the list's own, or for a
  // search result the one the scope picks.
  mailboxId: string;
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

type Member = [string, MemberState];

function senderOf(email: EmailHeader): string | null {
  const address = email.from?.[0] ?? email.sender?.[0];
  if (address === undefined) {
    return null;
  }
  return address.name === null || address.name === "" ? address.email : address.name;
}

// The members a scope covers in the thread's order, oldest first; the
// exemplar alone when the thread row names none there and the exemplar
// stands there.
export function membersInScope(
  email: EmailHeader,
  thread: ThreadRow | undefined,
  scope: Scope,
): Member[] {
  const states = new Map(Object.entries(thread?.members ?? {}));
  const inside = (thread?.emailIds ?? []).flatMap((id): Member[] => {
    const state = states.get(id);
    return state !== undefined && inScope(state, scope) ? [[id, state]] : [];
  });
  if (inside.length > 0) {
    return inside;
  }
  return inScope(email, scope) ? [[email.id, memberOf(email)]] : [];
}

function rowOf(email: EmailHeader, mailboxId: string, members: readonly Member[]): ListRow {
  return {
    id: email.id,
    threadId: email.threadId,
    mailboxId,
    sender: senderOf(email),
    subject: email.subject,
    preview: email.preview,
    receivedAt: email.receivedAt,
    unread: members.some(([, member]) => !(SEEN in member.keywords)),
    flagged: members.some(([, member]) => FLAGGED in member.keywords),
    hasAttachment: email.hasAttachment,
    memberIds: members.map(([memberId]) => memberId),
    count: members.length,
  };
}

// The rows of one page, in the server's order; an id without a header
// is left out, and so is a thread with no member left in the mailbox.
export function listRows(page: WindowPage, mailboxId: string): ListRow[] {
  const emails = new Map(Object.entries(page.emails));
  const threads = new Map(Object.entries(page.threads));
  const scope: Scope = { kind: "one", mailboxId };
  return page.ids.flatMap((id) => {
    const email = emails.get(id);
    if (email === undefined) {
      return [];
    }
    const members = membersInScope(email, threads.get(email.threadId), scope);
    return members.length === 0 ? [] : [rowOf(email, mailboxId, members)];
  });
}

// The mailbox a search result acts from: the scope's own where it covers
// one; else, among the mailboxes the scope covers that the exemplar
// stands in, the first in the tree's order, which draws the inbox first.
// An exemplar that left the scope gives way to the members still in it.
export function actingMailbox(
  exemplar: MemberState,
  members: readonly MemberState[],
  scope: Scope,
  places: TreePlaces,
): string | null {
  if (scope.kind === "one") {
    return scope.mailboxId;
  }
  const covered = (state: MemberState): string[] =>
    Object.keys(state.mailboxIds).filter((id) => covers(scope, id));
  const own = covered(exemplar);
  const candidates = own.length > 0 ? own : members.flatMap(covered);
  const place = (id: string): number => places.get(id) ?? Number.POSITIVE_INFINITY;
  const [first] = candidates.toSorted((a, b) => place(a) - place(b) || a.localeCompare(b));
  return first ?? null;
}

// The rows of one page of a search: each thread with a member the scope
// covers, read and acted on as the mailbox it acts from shows it.
export function resultRows(page: WindowPage, scope: Scope, places: TreePlaces): ListRow[] {
  const emails = new Map(Object.entries(page.emails));
  const threads = new Map(Object.entries(page.threads));
  return page.ids.flatMap((id) => {
    const email = emails.get(id);
    if (email === undefined) {
      return [];
    }
    const thread = threads.get(email.threadId);
    const inside = membersInScope(email, thread, scope);
    const exemplar = new Map(inside).get(id) ?? memberOf(email);
    const states = inside.map(([, state]) => state);
    const mailboxId = inside.length === 0 ? null : actingMailbox(exemplar, states, scope, places);
    if (mailboxId === null) {
      return [];
    }
    const members = membersInScope(email, thread, { kind: "one", mailboxId });
    return [rowOf(email, mailboxId, members)];
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
