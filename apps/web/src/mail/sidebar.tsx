// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow } from "@huliho/core";

import { Skeleton } from "../design-system/skeleton";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { AccountSwitcher } from "./account-switcher";
import { MailboxTree } from "./mailbox-tree";
import type { TreeState } from "./tree";
import styles from "./sidebar.module.css";

// Six still rows, the shape of the tree while it loads.
const SKELETON_ROW_COUNT = 6;

export interface SidebarProps {
  locale: Locale;
  accounts: readonly AccountRow[];
  account: AccountRow;
  tree: TreeState;
  currentMailboxId: string | undefined;
  showLetters: boolean;
  onNavigate?: (() => void) | undefined;
}

function TreeSkeleton({ locale }: { locale: Locale }) {
  return (
    <output className={styles.skeleton} aria-label={m.loading_label({}, { locale })}>
      {Array.from({ length: SKELETON_ROW_COUNT }, (_, index) => (
        <Skeleton key={index} className={styles.skeletonRow} />
      ))}
    </output>
  );
}

// The account switcher over the mailbox tree: the whole sidebar at the
// desktop width, the sheet's content at the other two.
export function Sidebar(props: SidebarProps) {
  const { locale, accounts, account, tree } = props;
  return (
    <nav className={styles.sidebar} aria-label={m.mail_sidebar({}, { locale })}>
      <AccountSwitcher
        locale={locale}
        accounts={accounts}
        account={account}
        variant="full"
        onNavigate={props.onNavigate}
      />
      {tree.status === "pending" && <TreeSkeleton locale={locale} />}
      {tree.status === "success" && (
        <MailboxTree
          locale={locale}
          accountId={account.id}
          mailboxes={tree.mailboxes}
          currentId={props.currentMailboxId}
          showLetters={props.showLetters}
          onNavigate={props.onNavigate}
        />
      )}
    </nav>
  );
}
