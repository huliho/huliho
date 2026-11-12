// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MailStore } from "@huliho/core";

// The store with every write held until the window reported the
// persistence request, so the request goes out before the first row lands.
export function heldUntil(store: MailStore, gate: Promise<unknown>): MailStore {
  return {
    mailboxes: (accountId) => store.mailboxes(accountId),
    emails: (accountId, ids) => store.emails(accountId, ids),
    threads: (accountId, ids) => store.threads(accountId, ids),
    query: (accountId, mailboxId) => store.query(accountId, mailboxId),
    queries: (accountId) => store.queries(accountId),
    search: (accountId, key) => store.search(accountId, key),
    searches: (accountId) => store.searches(accountId),
    senders: (accountId) => store.senders(accountId),
    state: (accountId, type) => store.state(accountId, type),
    body: (accountId, emailId) => store.body(accountId, emailId),
    bodySizes: () => store.bodySizes(),
    pending: (accountId) => store.pending(accountId),
    draft: (accountId, id) => store.draft(accountId, id),
    drafts: (accountId) => store.drafts(accountId),
    quota: (accountId) => store.quota(accountId),
    commit: async (accountId, batch) => {
      await gate;
      await store.commit(accountId, batch);
    },
  };
}
