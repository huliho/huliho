// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";
import {
  accountsQueryOptions,
  mailboxesQueryOptions,
  preferencesQueryOptions,
} from "@huliho/state";
import { useQuery } from "@tanstack/react-query";
import type { UseQueryResult } from "@tanstack/react-query";
import { Navigate, Outlet, useMatch, useParams, useSearch } from "@tanstack/react-router";
import { useEffect } from "react";

import { mailCache } from "../cache/client";
import { useMailCache } from "../cache/use-mail-cache";
import { useLocale } from "../i18n/locale";
import { useLayout } from "../shell/breakpoints";
import { DEFAULT_READING_PANE } from "../theme/appearance";
import { useJumpCommands } from "./jump-commands";
import { rememberLastAccount } from "./last-account";
import { prefetchMenus } from "./menus-chunk";
import { prefetchThreadPane } from "./reading-pane";
import { useSearchCommand } from "./search/open-search";
import { SEARCH_ROUTE_ID } from "./search/route-params";
import { ShellFrame } from "./shell-frame";
import type { TreeState } from "./tree";
import { useCloseThread } from "./use-close-thread";

function treeState(query: UseQueryResult<Mailbox[]>): TreeState {
  if (query.isSuccess) {
    return { status: "success", mailboxes: query.data };
  }
  if (query.isError) {
    return {
      status: "error",
      retry: () => {
        void query.refetch();
      },
    };
  }
  return { status: "pending" };
}

// The mailbox the screen shows (a search shows the one it was opened
// from) and whether the route is the search.
function useShown(mailboxId: string | undefined): { shownId?: string; searching: boolean } {
  const box = useSearch({ strict: false, select: (search) => search.box });
  const searching = useMatch({ from: SEARCH_ROUTE_ID, shouldThrow: false }) !== undefined;
  const shownId = mailboxId ?? box;
  return shownId === undefined ? { searching } : { shownId, searching };
}

// The mail screen of one account: it leases the worker the accounts of
// the session and the mailbox it shows, remembers the account for the
// next visit, registers the jump to each mailbox and the search and
// draws the frame around the route's pane, with the thread the address
// names where the reading pane preference puts it.
export function MailShell() {
  const locale = useLocale();
  const layout = useLayout();
  const { accountId } = useParams({ from: "/signed-in/mail/$accountId" });
  const { mailboxId, threadId } = useParams({ strict: false });
  const { shownId, searching } = useShown(mailboxId);
  // The mailbox a result open in the search acts from.
  const actsFrom = useSearch({ strict: false, select: (search) => search.at });
  const closeThread = useCloseThread(accountId, mailboxId);
  const list = useQuery(accountsQueryOptions);
  const tree = useQuery(mailboxesQueryOptions(mailCache, accountId));
  const preferences = useQuery(preferencesQueryOptions);
  useMailCache(shownId === undefined ? null : { accountId, mailboxId: shownId });
  useJumpCommands(tree.data ?? null, { accountId, mailboxId, locale });
  useSearchCommand(
    accountId,
    tree.data?.find((row) => row.id === shownId),
    locale,
    searching,
  );
  useEffect(() => {
    rememberLastAccount(accountId);
  }, [accountId]);
  // The chunks of the reading pane with the actions and of the menus are
  // fetched once the shell stands, ahead of the first use.
  useEffect(() => {
    void prefetchThreadPane();
    void prefetchMenus();
  }, []);
  const accounts = list.data?.accounts ?? [];
  const account = accounts.find((row) => row.id === accountId);
  if (account === undefined) {
    // The session let go of this account: the root picks another.
    return list.data === undefined ? null : <Navigate to="/" replace />;
  }
  return (
    <ShellFrame
      locale={locale}
      layout={layout}
      readingPane={preferences.data?.readingPane ?? DEFAULT_READING_PANE}
      cache={mailCache}
      accounts={accounts}
      account={account}
      tree={treeState(tree)}
      currentMailboxId={shownId}
      currentThreadId={threadId}
      searching={searching}
      actsFrom={actsFrom}
      onCloseThread={closeThread}
    >
      <Outlet />
    </ShellFrame>
  );
}
