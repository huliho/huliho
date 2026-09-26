// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow, MailCache } from "@huliho/core";
import { mailboxesQueryOptions } from "@huliho/state";
import { useQueries } from "@tanstack/react-query";

// The unread count of every account's inbox, from the trees the cache
// holds; an account whose tree is not in yet, or has no inbox, counts nothing.
export function useInboxCounts(
  cache: MailCache,
  accounts: readonly AccountRow[],
): ReadonlyMap<string, number> {
  return useQueries({
    queries: accounts.map((row) => mailboxesQueryOptions(cache, row.id)),
    combine: (results) =>
      new Map(
        results.flatMap((result, at): [string, number][] => {
          const id = accounts.at(at)?.id;
          const inbox = result.data?.find((mailbox) => mailbox.role === "inbox");
          return id === undefined || inbox === undefined ? [] : [[id, inbox.unreadEmails]];
        }),
      ),
  });
}
