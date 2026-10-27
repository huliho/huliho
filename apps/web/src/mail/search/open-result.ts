// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow } from "@huliho/core";
import { useNavigate } from "@tanstack/react-router";

import { deeperInSearch, markedForList } from "../thread-history";
import type { OpenFocus } from "../thread-history";
import type { SearchRoute } from "./route-params";

// The first result opened pushes an entry one deeper in the search, which
// closing goes back over; another result while one stands replaces it.
export function useOpenResult(
  accountId: string,
  route: SearchRoute,
  threadId: string | undefined,
): (row: ListRow, focus: OpenFocus) => void {
  const navigate = useNavigate();
  return (row, focus) => {
    void navigate({
      to: "/mail/$accountId/search/$threadId",
      params: { accountId, threadId: row.threadId },
      search: { ...route, at: row.mailboxId },
      replace: threadId !== undefined,
      state: (state) => {
        const entry = threadId === undefined ? deeperInSearch(state) : state;
        return focus === "list" ? markedForList(entry) : entry;
      },
    });
  };
}
