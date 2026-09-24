// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { WINDOW_SIZE } from "@huliho/core";
import type { ListPage, ListRow, MailCache } from "@huliho/core";
import { threadWindowQueryOptions } from "@huliho/state";
import { useQueries } from "@tanstack/react-query";
import type { UseQueryResult } from "@tanstack/react-query";

// The pages a list holds: the first one carries the total and the
// marker's count; every page that has landed contributes its rows.
export interface ListPages {
  first: UseQueryResult<ListPage> | undefined;
  rows: ReadonlyMap<number, ListRow[]>;
  // Fetches every page that failed once more; true when they all landed.
  retry: () => Promise<boolean>;
}

export function pageOf(index: number): number {
  return Math.floor(index / WINDOW_SIZE);
}

export function rowAt(rows: ReadonlyMap<number, ListRow[]>, index: number): ListRow | undefined {
  return rows.get(pageOf(index))?.[index % WINDOW_SIZE];
}

// Where the row with that id stands among the pages held; null when no
// held page has it.
export function indexOf(rows: ReadonlyMap<number, ListRow[]>, id: string): number | null {
  for (const [page, list] of rows) {
    const at = list.findIndex((row) => row.id === id);
    if (at !== -1) {
      return page * WINDOW_SIZE + at;
    }
  }
  return null;
}

// The pages in view plus the first: one query each, on the cache the
// caller runs. The worker makes a page's rows, so a landing costs the
// list only this map.
export function useThreadPages(
  cache: MailCache,
  accountId: string,
  mailboxId: string,
  wanted: readonly number[],
): ListPages {
  return useQueries({
    queries: wanted.map((page) => threadWindowQueryOptions(cache, accountId, mailboxId, page)),
    combine: (results) => ({
      first: results[0],
      rows: new Map(
        results.flatMap((result, at): [number, ListRow[]][] => {
          const page = wanted.at(at);
          if (page === undefined || result.data === undefined) {
            return [];
          }
          return [[page, result.data.rows]];
        }),
      ),
      retry: async () => {
        const again = await Promise.all(
          results.filter((result) => result.isError).map((result) => result.refetch()),
        );
        return again.every((result) => result.isSuccess);
      },
    }),
  });
}
