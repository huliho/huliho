// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { JmapClient } from "../jmap/client";
import { followChanges } from "./changes";
import { CHANGES_ROUNDS_MAX } from "./limits";
import { refreshFirstPage } from "./refresh";
import type { MailStore, QueryRow } from "./store";

// What a poll changed, for the tabs to invalidate: the mailbox tree, the
// lists by mailbox id and the threads by id.
export interface AppliedChanges {
  mailboxes: boolean;
  windows: string[];
  threads: string[];
}

// One poll: the changes of every type since the held states, then the
// first page of every watched list refreshed for the marker. A list
// nobody watches is dropped once it may have moved, so its next visit
// fetches it anew.
export async function applyChanges(
  client: JmapClient,
  store: MailStore,
  watched: readonly string[],
): Promise<AppliedChanges> {
  const folded = await followChanges(
    client,
    store,
    ["Mailbox", "Email", "Thread"],
    CHANGES_ROUNDS_MAX,
  );
  const windows = new Set(folded.lists);
  if (folded.listMoved && !folded.reset) {
    const rows = await store.queries(client.accountId);
    await rows.reduce(async (previous, row) => {
      await previous;
      await settle(client, store, row, watched.includes(row.id));
      windows.add(row.id);
    }, Promise.resolve());
  }
  return { mailboxes: folded.mailboxes, windows: [...windows], threads: folded.threads };
}

async function settle(
  client: JmapClient,
  store: MailStore,
  row: QueryRow,
  isWatched: boolean,
): Promise<void> {
  if (isWatched && row.pages.some((page) => page.page === 0)) {
    await refreshFirstPage(client, store, row);
    return;
  }
  await store.commit(client.accountId, { queries: { remove: [row.id] } });
}
