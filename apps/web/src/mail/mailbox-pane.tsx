// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { firstSyncOf } from "@huliho/core";
import type { ListRow, Mailbox } from "@huliho/core";
import { accountsQueryOptions, mailboxesQueryOptions } from "@huliho/state";
import { useQuery } from "@tanstack/react-query";
import { Link, Navigate, useLocation, useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, useRef, useState } from "react";
import type { ReactNode, Ref, RefObject } from "react";

import { useRetryAccount } from "../accounts/use-retry-account";
import { mailCache, pollCache } from "../cache/client";
import buttonStyles from "../design-system/button.module.css";
import { cx } from "../design-system/cx";
import { EmptyState } from "../design-system/empty-state";
import { useLocale } from "../i18n/locale";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { useOnline } from "../shell/use-online";
import { ListFoot } from "./list-foot";
import { OfflineBanner } from "./offline-banner";
import { markedFromMailbox, unmarkedForList, wantsListFocus } from "./thread-history";
import { ThreadList } from "./thread-list";
import type { ListHandle } from "./thread-list";
import { ThreadListBanner } from "./thread-list-banner";
import { useToday } from "./use-today";
import styles from "./mailbox-pane.module.css";

interface EmptyMailboxProps {
  locale: Locale;
  accountId: string;
  mailbox: Mailbox;
  mailboxes: readonly Mailbox[];
  // The box takes the focus when a control above it leaves with it.
  ref?: Ref<HTMLDivElement> | undefined;
}

// An empty inbox points at the archive, any other empty mailbox at the
// inbox; the link stays away when the account has no such mailbox.
export function EmptyMailbox({ locale, accountId, mailbox, mailboxes, ref }: EmptyMailboxProps) {
  const inbox = mailbox.role === "inbox";
  const target = mailboxes.find((row) => row.role === (inbox ? "archive" : "inbox"));
  return (
    <div ref={ref} tabIndex={-1} className={styles.empty}>
      <EmptyState
        message={
          inbox
            ? m.mail_empty_inbox({}, { locale })
            : m.mail_empty_mailbox({ mailbox: mailbox.name }, { locale })
        }
      />
      {target !== undefined && (
        <Link
          to="/mail/$accountId/$mailboxId"
          params={{ accountId, mailboxId: target.id }}
          className={cx(buttonStyles.button, buttonStyles.secondary)}
        >
          {inbox ? m.mail_open_archive({}, { locale }) : m.mail_open_inbox({}, { locale })}
        </Link>
      )}
    </div>
  );
}

// The banner over the list while the account is stopped, read from the
// accounts list the session holds, with the retry behind it. A pass
// hands the focus on before the banner leaves, so it is never lost; a
// retry answered with a rejected credential hands it to Reconnect.
function useAccountBanner(
  locale: Locale,
  accountId: string,
  online: boolean,
  onResumed: (held: boolean) => void,
): ReactNode {
  const list = useQuery(accountsQueryOptions);
  const box = useRef<HTMLDivElement>(null);
  // The account whose retry just turned its stop into an expired one.
  const [expiredBy, setExpiredBy] = useState<string | null>(null);
  // An answer that lands after a move to another account's mailbox
  // patches its row and moves nothing here. A pass is told whether the
  // banner holds the focus while its button still stands.
  const retry = useRetryAccount(locale, {
    onResumed: (id) => {
      if (id === accountId) {
        onResumed(box.current?.contains(document.activeElement) === true);
      }
    },
    onStillStopped: (id, cause) => {
      if (id === accountId && cause === "credentials") {
        setExpiredBy(id);
      }
    },
  });
  const account = list.data?.accounts.find((row) => row.id === accountId);
  if (list.data === undefined || account === undefined) {
    return null;
  }
  return (
    <ThreadListBanner
      key={account.id}
      ref={box}
      locale={locale}
      account={account}
      probeIntervalMinutes={list.data.probeIntervalMinutes}
      online={online}
      outcome={retry.outcomes[account.id]}
      onRetry={() => {
        retry.retry(account.id);
      }}
      takeFocus={expiredBy === account.id}
      onFocusTaken={() => {
        setExpiredBy(null);
      }}
    />
  );
}

// A row opened puts its thread in the address, where the frame draws
// it. The first open pushes a marked entry closing can go back over;
// another row while a thread is open replaces the entry and keeps its
// state, so a thread reached by its address stays unmarked.
function useOpenThread(
  accountId: string,
  mailboxId: string,
  threadId: string | undefined,
): (row: ListRow) => void {
  const navigate = useNavigate();
  return (row) => {
    void navigate({
      to: "/mail/$accountId/$mailboxId/$threadId",
      params: { accountId, mailboxId, threadId: row.threadId },
      replace: threadId !== undefined,
      state: threadId === undefined ? markedFromMailbox : true,
    });
  };
}

// Once a retry brought the account back, the rows refresh without
// waiting for the poll. A focus the banner held goes into the list, or
// to the empty state where there is no list; one that moved on stays.
function afterResume(list: ListHandle | null, empty: HTMLDivElement | null, held: boolean): void {
  pollCache();
  if (held) {
    focusList(list, empty);
  }
}

function focusList(list: ListHandle | null, empty: HTMLDivElement | null): void {
  if (list === null) {
    empty?.focus();
  } else {
    list.focus();
  }
}

interface JumpFocus {
  accountId: string;
  mailboxId: string;
  list: RefObject<ListHandle | null>;
  empty: RefObject<HTMLDivElement | null>;
}

// The focus follows a jump once the pane draws the mailbox the router
// already names. The entry then loses its mark, so only the jump itself
// moves the focus.
function useJumpFocus({ accountId, mailboxId, list, empty }: JumpFocus): void {
  const navigate = useNavigate();
  const path = `/mail/${accountId}/${mailboxId}`;
  const target = useLocation({
    select: (location) =>
      wantsListFocus(location.state) ? `${location.state.key ?? ""}:${location.pathname}` : null,
  });
  useEffect(() => {
    if (target === null || !target.endsWith(`:${path}`)) {
      return;
    }
    focusList(list.current, empty.current);
    void navigate({
      to: "/mail/$accountId/$mailboxId",
      params: { accountId, mailboxId },
      replace: true,
      state: unmarkedForList,
    });
  }, [target, path, accountId, mailboxId, list, empty, navigate]);
}

// The list pane of one mailbox. A mailbox the tree lacks goes back to
// the account's inbox; one the tree knows as empty says so without a
// fetch, with the foot the list has; every other one gets the list, a
// fresh one per mailbox so the cursor, the pages and the scroll start
// over with it. The banner of a stopped account stands over either.
export function MailboxPane() {
  const locale = useLocale();
  const today = useToday();
  const online = useOnline();
  const { accountId, mailboxId } = useParams({ from: "/signed-in/mail/$accountId/$mailboxId" });
  const { threadId } = useParams({ strict: false });
  const tree = useQuery(mailboxesQueryOptions(mailCache, accountId));
  const listRef = useRef<ListHandle>(null);
  const emptyRef = useRef<HTMLDivElement>(null);
  const banner = useAccountBanner(locale, accountId, online, (held) => {
    afterResume(listRef.current, emptyRef.current, held);
  });
  useJumpFocus({ accountId, mailboxId, list: listRef, empty: emptyRef });
  const open = useOpenThread(accountId, mailboxId, threadId);
  if (!tree.isSuccess) {
    return null;
  }
  const mailbox = tree.data.find((row) => row.id === mailboxId);
  if (mailbox === undefined) {
    return <Navigate to="/mail/$accountId" params={{ accountId }} replace />;
  }
  const empty = (
    <EmptyMailbox
      ref={emptyRef}
      locale={locale}
      accountId={accountId}
      mailbox={mailbox}
      mailboxes={tree.data}
    />
  );
  if (mailbox.totalEmails === 0 && firstSyncOf(mailbox) === null) {
    return (
      <>
        {banner}
        <OfflineBanner locale={locale} online={online} />
        {empty}
        <ListFoot locale={locale} progress={null} />
      </>
    );
  }
  return (
    <>
      {banner}
      <ThreadList
        ref={listRef}
        key={`${accountId}/${mailbox.id}`}
        locale={locale}
        today={today}
        cache={mailCache}
        accountId={accountId}
        mailbox={mailbox}
        online={online}
        openThreadId={threadId ?? null}
        empty={empty}
        onOpen={open}
      />
    </>
  );
}
