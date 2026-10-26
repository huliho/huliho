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
import { openedFromMailbox } from "../thread-history";
import { useToday } from "../use-today";
import { returnFocus } from "./open-search";
import { SEARCH_ROUTE_ID } from "./route-params";
import type { SearchRoute } from "./route-params";
import type { SearchParts } from "./search-body";
import { SearchPane } from "./search-pane";

// Closing goes back over the entry opening pushed, so Back and Escape
// land in the same place; a search reached by its address gives way to
// the mailbox it names, else the account's inbox.
function useCloseSearch(accountId: string, box: string | undefined): () => void {
  const navigate = useNavigate();
  const router = useRouter();
  const fromMailbox = useLocation({ select: (location) => openedFromMailbox(location.state) });
  return () => {
    returnFocus();
    if (fromMailbox) {
      router.history.back();
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
// replacing the route's params so the entry the open pushed stays one.
export function SearchView({ parts, loading }: SearchViewProps) {
  const locale = useLocale();
  const online = useOnline();
  const today = useToday();
  const navigate = useNavigate();
  const { accountId } = useParams({ from: SEARCH_ROUTE_ID });
  const route = useSearch({ from: SEARCH_ROUTE_ID });
  const tree = useQuery(mailboxesQueryOptions(mailCache, accountId));
  const strict = useQuery(sessionQueryOptions).data?.privacyStrict === true;
  const close = useCloseSearch(accountId, route.box);
  if (!tree.isSuccess) {
    return loading;
  }
  const replace = (next: SearchRoute): void => {
    void navigate({ to: ".", search: next, replace: true, state: (state) => state });
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
      onRoute={replace}
      onClose={close}
    />
  );
}
