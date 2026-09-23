// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailHeader, Mailbox } from "../jmap/schemas";
import type { ThreadRow } from "./store";
import type { WindowPage } from "./window";

// A thread as the reading pane opens it: its row and the headers the
// store holds for its emails.
export interface ThreadDetail {
  thread: ThreadRow;
  emails: Record<string, EmailHeader>;
}

// The cache as a client reads it, whatever runs it: the mailbox tree, a
// page of a list, a thread and the marker's action. A failure throws
// JmapError, so a caller reads the stop cause or the limit's name.
export interface MailCache {
  mailboxes(accountId: string): Promise<Mailbox[]>;
  window(accountId: string, mailboxId: string, page: number): Promise<WindowPage>;
  thread(accountId: string, threadId: string): Promise<ThreadDetail | null>;
  reveal(accountId: string, mailboxId: string): Promise<void>;
}
