// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ObjectType } from "../jmap/calls";
import type { EmailHeader, Mailbox, Thread } from "../jmap/schemas";

// The keywords and mailboxes of one email of a thread, so a list row
// reads its thread's unread and flagged state without every header.
export interface MemberState {
  keywords: Record<string, true>;
  mailboxIds: Record<string, true>;
}

export interface ThreadRow extends Thread {
  members: Record<string, MemberState>;
}

// The first page as the server orders it now, held back while new
// mail waits for the user to bring it in.
export interface FreshPage {
  ids: string[];
  total: number | null;
  queryState: string;
}

// One page the list handed out: the exemplars in the order served.
export interface Page {
  page: number;
  ids: string[];
}

// The list of one mailbox as the cache serves it: the pages it handed
// out plus what a refresh found since.
export interface QueryRow {
  // The mailbox id.
  id: string;
  queryState: string;
  total: number | null;
  pages: Page[];
  // The new exemplars the marker counts, in the server's order.
  pending: string[];
  fresh: FreshPage | null;
}

export type StoreArea = "mailboxes" | "emails" | "threads" | "queries";

// One atomic write: the areas that empty first, then what leaves, then
// what lands, then the states.
export interface Batch {
  reset?: readonly StoreArea[];
  mailboxes?: { put?: readonly Mailbox[]; remove?: readonly string[] };
  emails?: { put?: readonly EmailHeader[]; remove?: readonly string[] };
  threads?: { put?: readonly ThreadRow[]; remove?: readonly string[] };
  queries?: { put?: readonly QueryRow[]; remove?: readonly string[] };
  states?: Partial<Record<ObjectType, string | null>>;
}

// The cache contract, keyed by the Huliho account id and the object id.
// Every row read is a copy the caller may keep; every write is one
// batch that lands whole or not at all.
export interface MailStore {
  mailboxes(accountId: string): Promise<Mailbox[]>;
  emails(accountId: string, ids: readonly string[]): Promise<Map<string, EmailHeader>>;
  threads(accountId: string, ids: readonly string[]): Promise<Map<string, ThreadRow>>;
  query(accountId: string, mailboxId: string): Promise<QueryRow | null>;
  queries(accountId: string): Promise<QueryRow[]>;
  state(accountId: string, type: ObjectType): Promise<string | null>;
  commit(accountId: string, batch: Batch): Promise<void>;
}
