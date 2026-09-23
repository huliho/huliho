// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";
import { accountsQueryOptions, mailboxesQueryOptions } from "@huliho/state";
import { useQuery } from "@tanstack/react-query";
import type { UseQueryResult } from "@tanstack/react-query";
import { Navigate, Outlet, useParams } from "@tanstack/react-router";
import { useEffect } from "react";

import { mailCache } from "../cache/client";
import { useMailCache } from "../cache/use-mail-cache";
import { useLocale } from "../i18n/locale";
import { useLayout } from "../shell/breakpoints";
import { rememberLastAccount } from "./last-account";
import { ShellFrame } from "./shell-frame";
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
// next visit and draws the frame around the route's pane.
export function MailShell() {
  const locale = useLocale();
  const layout = useLayout();
  const { accountId } = useParams({ from: "/signed-in/mail/$accountId" });
  const { mailboxId } = useParams({ strict: false });
  const list = useQuery(accountsQueryOptions);
  const tree = useQuery(mailboxesQueryOptions(mailCache, accountId));
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
  return (
    <ShellFrame
      locale={locale}
      layout={layout}
      accounts={accounts}
      account={account}
      tree={treeState(tree)}
      currentMailboxId={mailboxId}
    >
      <Outlet />
    </ShellFrame>
  );
}
