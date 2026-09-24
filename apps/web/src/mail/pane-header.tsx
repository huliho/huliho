// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow, Mailbox } from "@huliho/core";

import { Avatar } from "../design-system/avatar";
import { Skeleton } from "../design-system/skeleton";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { MonoCount, countWords } from "./mono-count";
import { countOf } from "./tree";
import styles from "./pane-header.module.css";

interface PaneHeaderProps {
  locale: Locale;
  account: AccountRow;
  // The open mailbox; undefined while the tree loads or resolves the inbox.
  mailbox: Mailbox | undefined;
  pending: boolean;
  // Present at the phone width, where the avatar opens the sidebar.
  onOpenSidebar?: (() => void) | undefined;
}

// The bar above the list: the mailbox's name and unread count, the
// count worded for a reader who cannot see what it stands beside.
export function PaneHeader({ locale, account, mailbox, pending, onOpenSidebar }: PaneHeaderProps) {
  const count = mailbox === undefined ? 0 : countOf(mailbox);
  return (
    <header className={styles.header}>
      {onOpenSidebar !== undefined && (
        <button
          type="button"
          className={styles.sidebarButton}
          aria-label={m.mail_sidebar({}, { locale })}
          onClick={onOpenSidebar}
        >
          <Avatar name={account.name} locale={locale} />
        </button>
      )}
      {mailbox !== undefined && <h1 className={styles.title}>{mailbox.name}</h1>}
      {mailbox !== undefined && count > 0 && (
        <MonoCount
          value={count}
          locale={locale}
          tone="accent"
          label={countWords(mailbox, locale)}
        />
      )}
      {mailbox === undefined && pending && <Skeleton className={styles.titleSkeleton} />}
    </header>
  );
}
