// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ThreadDetail } from "./api";
import type { MailStore } from "./store";

// The thread as the store holds it, with the headers it has for the
// members; null for a thread the store does not know.
export async function readThread(
  store: MailStore,
  accountId: string,
  threadId: string,
): Promise<ThreadDetail | null> {
  const thread = (await store.threads(accountId, [threadId])).get(threadId);
  if (thread === undefined) {
    return null;
  }
  const emails = await store.emails(accountId, thread.emailIds);
  return { thread, emails: Object.fromEntries(emails) };
}
