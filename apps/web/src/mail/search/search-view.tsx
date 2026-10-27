// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { mailboxesQueryOptions, sessionQueryOptions } from "@huliho/state";
import { useQuery } from "@tanstack/react-query";
import { useLocation, useNavigate, useParams, useRouter, useSearch } from "@tanstack/react-router";
import type { ReactNode } from "react";

import { mailCache } from "../../cache/client";
import { useLocale } from "../../i18n/locale";
import { useOnline } from "../../shell/use-online";
import { searchDepthOf } from "../thread-history";
import { useCloseThread } from "../use-close-thread";
import { useToday } from "../use-today";
import { useOpenResult } from "./open-result";
import { returnFocus } from "./open-search";
import { SEARCH_ROUTE_ID, searchRoute } from "./route-params";
import type { SearchRoute } from "./route-params";
import type { SearchParts } from "./search-body";
import { SearchPane } from "./search-pane";

// Closing goes back over the entries the search pushed, an open result's
// with its own, so Back and Escape land in the same place; a search
// reached by its address gives way to the mailbox it names, else the
// account's inbox.
function useCloseSearch(accountId: string, box: string | undefined): () => void {
  const navigate = useNavigate();
  const router = useRouter();
  const depth = useLocation({ select: (location) => searchDepthOf(location.state) });
  return () => {
    returnFocus();
    if (depth > 0) {
      router.history.go(-depth);
    } else if (box === undefined) {
      void navigate({ to: "/mail/$accountId", params: { accountId }, replace: true });
    } else {
      void navigate({
        to: "/mail/$accountId/$mailboxId",
        params: { accountId, mailboxId: box },
        replace: true,
      });
    }
  };
}

interface SearchViewProps {
  parts: SearchParts;
  loading: ReactNode;
}

// The search route's view: the pane over the account's tree, a run
// replacing the entry it stands on with the results of the new params,
// so the entry the open pushed stays one and an open result closes. A
// result opens and closes as a thread of a mailbox does, its address
// keeping the search's params.
export function SearchView({ parts, loading }: SearchViewProps) {
  const locale = useLocale();
  const online = useOnline();
  const today = useToday();
  const navigate = useNavigate();
  const { accountId } = useParams({ from: SEARCH_ROUTE_ID });
  const { threadId } = useParams({ strict: false });
  const route = useSearch({ from: SEARCH_ROUTE_ID });
  const actsFrom = useSearch({ strict: false, select: (search) => search.at });
  const params = searchRoute(route);
  const onOpen = useOpenResult(accountId, params, threadId);
  const onCloseResult = useCloseThread(accountId, undefined);
  const tree = useQuery(mailboxesQueryOptions(mailCache, accountId));
  const strict = useQuery(sessionQueryOptions).data?.privacyStrict === true;
  const close = useCloseSearch(accountId, route.box);
  if (!tree.isSuccess) {
    return loading;
  }
  const replace = (next: SearchRoute): void => {
    void navigate({
      to: "/mail/$accountId/search",
      params: { accountId },
      search: next,
      replace: true,
      state: (state) => state,
    });
  };
  return (
    <SearchPane
      locale={locale}
      cache={mailCache}
      accountId={accountId}
      route={route}
      mailboxes={tree.data}
      strict={strict}
      online={online}
      today={today}
      parts={parts}
      loading={loading}
      result={{
        threadId: threadId ?? null,
        actsFrom: actsFrom ?? null,
        onOpen,
        onClose: onCloseResult,
      }}
      onRoute={replace}
      onClose={close}
    />
  );
}
