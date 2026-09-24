// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow } from "@huliho/core";
import { Link } from "@tanstack/react-router";
import { Archive, FilePen, FolderTree, Inbox, Send, ShieldAlert, Trash2 } from "lucide-react";
import type { LucideIcon } from "lucide-react";

import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { AccountSwitcher } from "./account-switcher";
import { MonoCount, countLabel } from "./mono-count";
import { buildTree, countOf } from "./tree";
import type { TreeState } from "./tree";
import styles from "./rail.module.css";

const ROLE_ICONS = new Map<string, LucideIcon>([
  ["inbox", Inbox],
  ["drafts", FilePen],
  ["sent", Send],
  ["archive", Archive],
  ["junk", ShieldAlert],
  ["trash", Trash2],
]);

interface RailProps {
  locale: Locale;
  accounts: readonly AccountRow[];
  account: AccountRow;
  tree: TreeState;
  currentMailboxId: string | undefined;
  // Opens the whole sidebar, folders included.
  onMore: () => void;
}

// The sidebar collapsed to a rail: the avatar, the roles as icons and a
// button for the rest.
export function Rail({ locale, accounts, account, tree, currentMailboxId, onMore }: RailProps) {
  const roles = tree.status === "success" ? buildTree(tree.mailboxes, locale).roles : [];
  return (
    <nav className={styles.rail} aria-label={m.mail_sidebar({}, { locale })}>
      <AccountSwitcher locale={locale} accounts={accounts} account={account} variant="avatar" />
      {roles.map(({ mailbox }) => {
        const Icon = ROLE_ICONS.get(mailbox.role ?? "") ?? FolderTree;
        const count = countOf(mailbox);
        const current = mailbox.id === currentMailboxId;
        return (
          <Link
            key={mailbox.id}
            to="/mail/$accountId/$mailboxId"
            params={{ accountId: account.id, mailboxId: mailbox.id }}
            activeOptions={{ exact: true }}
            className={styles.item}
            aria-label={countLabel(mailbox, locale)}
          >
            <Icon className={styles.icon} aria-hidden="true" />
            {count > 0 && (
              <MonoCount value={count} locale={locale} tone={current ? "accent" : "muted"} />
            )}
          </Link>
        );
      })}
      <button
        type="button"
        className={styles.item}
        aria-label={m.mail_all_mailboxes({}, { locale })}
        onClick={onMore}
      >
        <FolderTree className={styles.icon} aria-hidden="true" />
      </button>
    </nav>
  );
}
