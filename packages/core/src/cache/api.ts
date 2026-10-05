// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { DownloadSource } from "../download";
import type { EmailHeader, Mailbox } from "../jmap/schemas";
import type { Mutation } from "./pending";
import type { ListPage } from "./rows";
import type { EmailBody, ThreadRow } from "./store";

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

// The cache as a client reads it, whatever runs it: the mailbox tree, a
// page of a list, a thread, the marker's action, the body of one email
// and one change to an email. A failure throws JmapError, so a caller
// reads the stop cause or the limit's name.
export interface MailCache {
  mailboxes(accountId: string): Promise<Mailbox[]>;
  window(accountId: string, mailboxId: string, page: number): Promise<ListPage>;
  thread(accountId: string, threadId: string): Promise<ThreadDetail | null>;
  reveal(accountId: string, mailboxId: string): Promise<void>;
  // `large` asks again at the large cap for a body the first ask cut
  // short. Null for an email the server does not have.
  body(accountId: string, emailId: string, options: { large: boolean }): Promise<BodyDetail | null>;
  // The rows take the change at once and the server hears it behind
  // them; a change the account or a mailbox refuses moves nothing.
  mutate(accountId: string, mutation: Mutation): Promise<void>;
}
