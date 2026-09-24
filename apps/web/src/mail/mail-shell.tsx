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
import {
  Navigate,
  Outlet,
  useLocation,
  useNavigate,
  useParams,
  useRouter,
} from "@tanstack/react-router";
import { useEffect } from "react";

import { mailCache } from "../cache/client";
import { useMailCache } from "../cache/use-mail-cache";
import { useLocale } from "../i18n/locale";
import { useLayout } from "../shell/breakpoints";
import { DEFAULT_READING_PANE } from "../theme/appearance";
import { rememberLastAccount } from "./last-account";
import { ShellFrame } from "./shell-frame";
import { openedFromMailbox } from "./thread-history";
import type { TreeState } from "./tree";

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

// The mail screen of one account: it leases the worker the accounts of
// the session and the mailbox it shows, remembers the account for the
// next visit and draws the frame around the route's pane, with the
// thread the address names where the reading pane preference puts it.
export function MailShell() {
  const locale = useLocale();
  const layout = useLayout();
  const navigate = useNavigate();
  const router = useRouter();
  const fromMailbox = useLocation({ select: (location) => openedFromMailbox(location.state) });
  const { accountId } = useParams({ from: "/signed-in/mail/$accountId" });
  const { mailboxId, threadId } = useParams({ strict: false });
  const list = useQuery(accountsQueryOptions);
  const tree = useQuery(mailboxesQueryOptions(mailCache, accountId));
  const preferences = useQuery(preferencesQueryOptions);
  useMailCache(mailboxId === undefined ? null : { accountId, mailboxId });
  useEffect(() => {
    rememberLastAccount(accountId);
  }, [accountId]);
  const accounts = list.data?.accounts ?? [];
  const account = accounts.find((row) => row.id === accountId);
  if (account === undefined) {
    // The session let go of this account: the root picks another.
    return list.data === undefined ? null : <Navigate to="/" replace />;
  }
  // Closing goes back over the entry the list pushed, so the history
  // reads as it did before the open; a thread reached by its address
  // leaves it in place instead.
  const closeThread = (): void => {
    if (mailboxId === undefined) {
      return;
    }
    if (fromMailbox) {
      router.history.back();
      return;
    }
    void navigate({
      to: "/mail/$accountId/$mailboxId",
      params: { accountId, mailboxId },
      replace: true,
    });
  };
  return (
    <ShellFrame
      locale={locale}
      layout={layout}
      readingPane={preferences.data?.readingPane ?? DEFAULT_READING_PANE}
      cache={mailCache}
      accounts={accounts}
      account={account}
      tree={treeState(tree)}
      currentMailboxId={mailboxId}
      currentThreadId={threadId}
      onCloseThread={closeThread}
    >
      <Outlet />
    </ShellFrame>
  );
}
