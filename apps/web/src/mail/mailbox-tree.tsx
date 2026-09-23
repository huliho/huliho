// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";
import { Link } from "@tanstack/react-router";
import type { KeyboardEvent } from "react";

import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { buildTree, countOf, flatten } from "./tree";
import type { FlatRow } from "./tree";
import { UnreadCount, countLabel } from "./unread-count";
import styles from "./mailbox-tree.module.css";

interface MailboxTreeProps {
  locale: Locale;
  accountId: string;
  mailboxes: readonly Mailbox[];
  currentId: string | undefined;
  // The jump letters show where a keyboard is likely: the desktop layout.
  showLetters: boolean;
  onNavigate?: (() => void) | undefined;
}

interface ItemProps extends Omit<MailboxTreeProps, "mailboxes"> {
  row: FlatRow;
  // The one item Tab lands on: the open mailbox, else the first.
  stopId: string | undefined;
}

const ITEM_SELECTOR = '[role="treeitem"]';

// The item an arrow, Home or End lands on; null for any other key.
function targetIndex(key: string, index: number, count: number): number | null {
  switch (key) {
    case "ArrowDown":
      return Math.min(count - 1, index + 1);
    case "ArrowUp":
      return Math.max(0, index - 1);
    case "Home":
      return 0;
    case "End":
      return count - 1;
    default:
      return null;
  }
}

// Focus moves within the tree on the arrow keys and Tab leaves it, so
// the whole tree is one stop on the way through the shell.
function moveFocus(event: KeyboardEvent<HTMLUListElement>): void {
  const items = [...event.currentTarget.querySelectorAll<HTMLElement>(ITEM_SELECTOR)];
  const index = items.findIndex((item) => item === event.target);
  const next = index === -1 ? null : targetIndex(event.key, index, items.length);
  if (next === null) {
    return;
  }
  event.preventDefault();
  items.at(next)?.focus();
}

// A nested row moves in by one step per level below the first.
function indent(level: number): string {
  return `calc(var(--hhx-space-3) + ${String(level - 1)} * var(--hhx-space-4))`;
}

function TreeItem({ row, stopId, ...props }: ItemProps) {
  const { mailbox } = row;
  const current = mailbox.id === props.currentId;
  const count = countOf(mailbox);
  return (
    <li role="none">
      <Link
        role="treeitem"
        to="/mail/$accountId/$mailboxId"
        params={{ accountId: props.accountId, mailboxId: mailbox.id }}
        activeOptions={{ exact: true }}
        className={styles.row}
        style={row.level > 1 ? { paddingInlineStart: indent(row.level) } : undefined}
        aria-level={row.level}
        aria-posinset={row.position}
        aria-setsize={row.size}
        aria-label={countLabel(mailbox, props.locale)}
        tabIndex={mailbox.id === stopId ? 0 : -1}
        onClick={props.onNavigate}
      >
        <span className={styles.name}>{mailbox.name}</span>
        {props.showLetters && row.letter !== null && (
          <span className={styles.letter} aria-hidden="true">
            {row.letter}
          </span>
        )}
        {count > 0 && (
          <UnreadCount value={count} locale={props.locale} tone={current ? "accent" : "muted"} />
        )}
      </Link>
    </li>
  );
}

// The mailboxes of one account as a navigation tree: the roles in their
// fixed order, then the folders with their depth on each row; one tab
// stop with the arrow keys inside.
export function MailboxTree({ mailboxes, ...props }: MailboxTreeProps) {
  const { locale } = props;
  const model = buildTree(mailboxes, locale);
  const roles = flatten(model.roles);
  const folders = flatten(model.folders);
  const held = mailboxes.some((row) => row.id === props.currentId);
  const stopId = held ? props.currentId : (roles[0] ?? folders[0])?.mailbox.id;
  return (
    <ul
      role="tree"
      aria-label={m.mail_mailboxes({}, { locale })}
      className={styles.tree}
      onKeyDown={moveFocus}
    >
      {roles.map((row) => (
        <TreeItem key={row.mailbox.id} row={row} stopId={stopId} {...props} />
      ))}
      {folders.length > 0 && (
        <li role="none">
          <span className={styles.groupLabel} aria-hidden="true">
            {m.tree_folders({}, { locale })}
          </span>
        </li>
      )}
      {folders.map((row) => (
        <TreeItem key={row.mailbox.id} row={row} stopId={stopId} {...props} />
      ))}
    </ul>
  );
}
