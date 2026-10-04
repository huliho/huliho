// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Authentication } from "../auth-results";
import type { EmailBodyPart, EmailBodyValue } from "../jmap/body";
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

// The body of one email as the proxy answered it, its HTML values
// sanitized there. The window builds what it renders from this row and
// stores nothing of that.
export interface EmailBody {
  // The email id.
  id: string;
  bodyStructure: EmailBodyPart;
  textBody: EmailBodyPart[];
  htmlBody: EmailBodyPart[];
  attachments: EmailBodyPart[];
  bodyValues: Record<string, EmailBodyValue>;
  // What the topmost Authentication-Results header says.
  authentication: Authentication;
  // Set when the message is one text part in the flowed format.
  flowed: { delSp: boolean } | null;
  // Whether the values were asked at the large cap.
  large: boolean;
  fetchedAt: number;
  // The weight of the values, which the eviction counts.
  bytes: number;
}

// Where one body stands in the eviction's order.
export interface BodySize {
  accountId: string;
  id: string;
  fetchedAt: number;
  bytes: number;
}

// The keywords an email gains (true) and loses (null), by the path a
// /set update names them with (RFC 8620 section 5.3).
export type EmailPatch = Record<string, true | null>;

// One change the server has not acknowledged: the patch the rows took
// and the patch that takes it back.
export interface PendingRow {
  seq: number;
  type: "Email";
  // The email id.
  id: string;
  patch: EmailPatch;
  inverse: EmailPatch;
  // When a request last carried the patch, null before the first.
  sentAt: number | null;
}

export type StoreArea = "mailboxes" | "emails" | "threads" | "queries" | "bodies" | "pending";

// One atomic write: the areas that empty first, then what leaves, then
// what lands, then the states.
export interface Batch {
  reset?: readonly StoreArea[];
  mailboxes?: { put?: readonly Mailbox[]; remove?: readonly string[] };
  emails?: { put?: readonly EmailHeader[]; remove?: readonly string[] };
  threads?: { put?: readonly ThreadRow[]; remove?: readonly string[] };
  queries?: { put?: readonly QueryRow[]; remove?: readonly string[] };
  bodies?: { put?: readonly EmailBody[]; remove?: readonly string[] };
  // A pending row leaves by its sequence number.
  pending?: { put?: readonly PendingRow[]; remove?: readonly number[] };
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
  body(accountId: string, emailId: string): Promise<EmailBody | null>;
  // Every body of every account, without its values.
  bodySizes(): Promise<BodySize[]>;
  // The unacknowledged changes of the account, oldest first.
  pending(accountId: string): Promise<PendingRow[]>;
  commit(accountId: string, batch: Batch): Promise<void>;
}
