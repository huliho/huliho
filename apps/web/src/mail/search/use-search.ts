// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { searchKeyOf } from "@huliho/core";
import type { ListPage, MailCache, SearchParams } from "@huliho/core";
import { queryKeys, searchWindowQueryOptions } from "@huliho/state";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { UseQueryResult } from "@tanstack/react-query";
import { useEffect, useState } from "react";

import { noteSearch } from "./recent-searches";
import { searchRoute } from "./route-params";
import type { SearchRoute } from "./route-params";
import { holdsNothing } from "./search-draft";

// A search past this says "Searching…", the progress any wait gets.
export const SEARCHING_AFTER_MS = 300;

// A search past this says a server without an index takes a while,
// with Cancel beside it.
export const SEARCH_SLOW_MS = 4000;

// Where a search in flight stands: quiet at first, then searching, then slow.
type Phase = "quiet" | "searching" | "slow";

function usePhase(fetching: boolean): Phase {
  const [phase, setPhase] = useState<Phase>("quiet");
  useEffect(() => {
    if (!fetching) {
      return undefined;
    }
    const searching = setTimeout(() => {
      setPhase("searching");
    }, SEARCHING_AFTER_MS);
    const slow = setTimeout(() => {
      setPhase("slow");
    }, SEARCH_SLOW_MS);
    return () => {
      clearTimeout(searching);
      clearTimeout(slow);
      setPhase("quiet");
    };
  }, [fetching]);
  return fetching ? phase : "quiet";
}

// What a run reads: the account's cache, the tab's locale, whether the
// network is there, where the recents live and how the route moves.
export interface RunContext {
  cache: MailCache;
  accountId: string;
  locale: string;
  online: boolean;
  strict: boolean;
  route: SearchRoute;
  onRoute: (route: SearchRoute) => void;
}

// The search the route holds and what can be done to it.
export interface RanSearch {
  key: string | null;
  page: UseQueryResult<ListPage>;
  phase: Phase;
  canceled: boolean;
  run: (params: SearchParams) => void;
  cancel: () => void;
}

// The route's params as the URL writes them, from the key's own form.
function routeOf(params: SearchParams, box: string | undefined): SearchRoute {
  const parsed: unknown = JSON.parse(searchKeyOf(params));
  const fields = typeof parsed === "object" && parsed !== null ? Object.entries(parsed) : [];
  return { ...searchRoute(Object.fromEntries(fields)), ...(box === undefined ? {} : { box }) };
}

// The search in the route, its first page and the run: a run asks the
// server anew and replaces the route's params, the later pages of the
// search read again once the new first page is in; offline it replaces
// the params alone. The route's search joins the recents while the tab
// is online, however it got there; a run notes it again, so a repeat
// moves up. Cancel gives the request up and keeps the field.
export function useRanSearch(context: RunContext, ran: SearchParams): RanSearch {
  const { cache, accountId, locale, online, strict } = context;
  const queryClient = useQueryClient();
  const key = holdsNothing(ran) ? null : searchKeyOf(ran);
  const [canceled, setCanceled] = useState<string | null>(null);
  const page = useQuery({
    ...searchWindowQueryOptions(cache, accountId, key ?? "", { locale, page: 0 }),
    enabled: key !== null && online,
  });
  useEffect(() => {
    if (key !== null && online) {
      noteSearch(accountId, key, strict);
    }
  }, [accountId, key, online, strict]);
  const run = (params: SearchParams): void => {
    if (holdsNothing(params)) {
      return;
    }
    const next = searchKeyOf(params);
    setCanceled(null);
    if (online) {
      noteSearch(accountId, next, strict);
      const fresh = searchWindowQueryOptions(cache, accountId, next, {
        locale,
        page: 0,
        again: true,
      });
      // The page's own query shows the outcome; this promise has no reader.
      // The run filled the first page, so the pages after it read again.
      queryClient
        .query({ ...fresh, staleTime: 0 })
        .then(() =>
          queryClient.invalidateQueries({
            queryKey: queryKeys.searchPages(accountId, next),
            predicate: (query) => query.queryKey.at(-1) !== 0,
          }),
        )
        .catch(() => undefined);
    }
    context.onRoute(routeOf(params, context.route.box));
  };
  const cancel = (): void => {
    if (key !== null) {
      setCanceled(key);
      void queryClient.cancelQueries({ queryKey: queryKeys.search(accountId, key, locale, 0) });
    }
  };
  const phase = usePhase(page.fetchStatus === "fetching");
  return { key, page, phase, canceled: key !== null && canceled === key, run, cancel };
}
