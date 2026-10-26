// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { DownloadSource } from "../download";
import type { EmailHeader, Mailbox } from "../jmap/schemas";
import type { Job } from "./jobs";
import type { Mutation, PatchMutation } from "./pending";
import type { QuotaReading } from "./quota";
import type { ListPage } from "./rows";
import type { EmailBody, Sender, ThreadRow } from "./store";

// A thread as the reading pane opens it: its row and the headers the
// store holds for its emails.
export interface ThreadDetail {
  thread: ThreadRow;
  emails: Record<string, EmailHeader>;
}

// The body of one email with where its parts download from.
export interface BodyDetail {
  body: EmailBody;
  download: DownloadSource;
}

// What takes some changes back: the changes that restore each email,
// the held destroys by sequence number and the threads that moved.
export interface Undo {
  inverses: PatchMutation[];
  held: number[];
  threads: string[];
}

// How a page of a search is asked: the locale the tree's names sort in,
// whether the search runs anew and a signal that gives the request up.
export interface SearchCall {
  locale: string;
  again?: boolean;
  signal?: AbortSignal;
}

// The cache as a client reads it, whatever runs it: the mailbox tree, a
// page of a list or of a search, the senders of the held mail, a thread,
// the marker's action, the body of one email, the changes to emails, the
// folder jobs and the quota. A failure throws JmapError, so a caller
// reads the stop cause or the limit's name; a method the server refused
// throws MethodFailure with its error type.
export interface MailCache {
  mailboxes(accountId: string): Promise<Mailbox[]>;
  window(accountId: string, mailboxId: string, page: number): Promise<ListPage>;
  // A page of the search the key names, as `searchKeyOf` writes it; a
  // search given up rejects with the signal's reason.
  search(accountId: string, key: string, page: number, call: SearchCall): Promise<ListPage>;
  // The senders whose name or address starts with the prefix, most mail first.
  senders(accountId: string, prefix: string): Promise<Sender[]>;
  thread(accountId: string, threadId: string): Promise<ThreadDetail | null>;
  reveal(accountId: string, mailboxId: string): Promise<void>;
  // `large` asks again at the large cap for a body the first ask cut
  // short. Null for an email the server does not have.
  body(accountId: string, emailId: string, options: { large: boolean }): Promise<BodyDetail | null>;
  // The rows take the change at once and the server hears it behind
  // them; a change the account or a mailbox refuses moves nothing.
  mutate(accountId: string, mutation: PatchMutation): Promise<void>;
  // Several changes at once, as `mutate` takes one; a destroy waits held
  // until `release` lets it go or `drop` takes it back.
  mutateMany(accountId: string, mutations: readonly Mutation[]): Promise<Undo>;
  release(accountId: string, held: readonly number[]): Promise<void>;
  drop(accountId: string, held: readonly number[]): Promise<void>;
  // Starts a folder job, one per account at a time; its progress reaches
  // every tab. A job asked while one of the account runs is left alone
  // and the call resolves all the same. `cancelJob` ends the running job
  // after the round that runs.
  job(accountId: string, job: Job): Promise<void>;
  cancelJob(accountId: string): Promise<void>;
  // The quota the bar shows, null where the account reports none.
  quota(accountId: string): Promise<QuotaReading | null>;
}
