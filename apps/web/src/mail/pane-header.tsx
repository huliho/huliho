// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow, Mailbox } from "@huliho/core";
import { Suspense, use } from "react";

import { useRegistered } from "../commands/use-command";
import { Avatar } from "../design-system/avatar";
import { Skeleton } from "../design-system/skeleton";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { useCoarsePointer } from "../shell/pointer";
import { startSelection } from "./actions/selection-lend";
import type { FolderMenuProps } from "./folder-menu";
import { prefetchMenus } from "./menus-chunk";
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

function LoadedFolderMenu(props: FolderMenuProps) {
  const { FolderMenu } = use(prefetchMenus());
  return <FolderMenu {...props} />;
}

// The mailbox's name, a menu button on touch while the list lets threads
// be picked: its Select is the visible route beside the long press.
function Title({ locale, mailbox }: { locale: Locale; mailbox: Mailbox }) {
  const touch = useCoarsePointer();
  const pickable = useRegistered().some((command) => command.id === "selection.toggle");
  return (
    <h1 className={styles.title}>
      {touch && pickable ? (
        <Suspense fallback={mailbox.name}>
          <LoadedFolderMenu
            name={mailbox.name}
            select={{
              label: m.command_selection({ action: "select" }, { locale }),
              run: startSelection,
            }}
          />
        </Suspense>
      ) : (
        mailbox.name
      )}
    </h1>
  );
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
      {mailbox !== undefined && <Title locale={locale} mailbox={mailbox} />}
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
