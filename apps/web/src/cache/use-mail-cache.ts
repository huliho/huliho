// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { accountsQueryOptions } from "@huliho/state";
import { useQuery } from "@tanstack/react-query";
import { useEffect } from "react";

import { attachCache } from "./client";
import type { Watch } from "./coordinator";

// Runs the cache for the accounts the session holds while the shell is
// mounted; `watching` names the mailbox this tab shows, if any.
export function useMailCache(watching: Watch | null): void {
  const { data, dataUpdatedAt } = useQuery(accountsQueryOptions);
  const watchedAccount = watching?.accountId ?? null;
  const watchedMailbox = watching?.mailboxId ?? null;
  useEffect(() => {
    const watch =
      watchedAccount === null || watchedMailbox === null
        ? null
        : { accountId: watchedAccount, mailboxId: watchedMailbox };
    return data === undefined
      ? undefined
      : attachCache({
          accounts: data.accounts.map((row) => row.id),
          listedAt: dataUpdatedAt,
          watching: watch,
        });
  }, [data, dataUpdatedAt, watchedAccount, watchedMailbox]);
}
