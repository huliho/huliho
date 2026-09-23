// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { JmapClient } from "../jmap/client";
import { followChanges, hydrateMailboxes } from "./changes";
import { CHANGES_ROUNDS_MAX } from "./limits";
import type { MailStore } from "./store";

// The account's mailboxes brought up to date: every one of them at the
// first call, the changes since the held state afterwards. True when a
// mailbox row changed.
export async function syncMailboxes(client: JmapClient, store: MailStore): Promise<boolean> {
  if ((await store.state(client.accountId, "Mailbox")) === null) {
    await hydrateMailboxes(client, store);
    return true;
  }
  const folded = await followChanges(client, store, ["Mailbox"], CHANGES_ROUNDS_MAX);
  return folded.mailboxes;
}
