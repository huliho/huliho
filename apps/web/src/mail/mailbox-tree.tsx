// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { visibleMailboxes } from "@huliho/core";
import type { MailCache, Mailbox } from "@huliho/core";
import { Link } from "@tanstack/react-router";
import { FolderPlus } from "lucide-react";
import { useLayoutEffect, useRef, useState } from "react";
import type { FocusEvent, KeyboardEvent, RefObject } from "react";

import type { Chord } from "../commands/keys";
import { cx } from "../design-system/cx";
import iconButton from "../design-system/icon-button.module.css";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { askNewFolder } from "./actions/folder-items";
import type { FolderShell } from "./actions/folder-items";
import { HintedButton } from "./hinted-button";
import { focusLost } from "./list-cursor";
import { MonoCount, countLabel } from "./mono-count";
import { buildTree, countOf, flatten } from "./tree";
import type { FlatRow } from "./tree";
import { useTreeMenu } from "./tree-menu";
import styles from "./mailbox-tree.module.css";

interface MailboxTreeProps {
  locale: Locale;
  cache: MailCache;
  accountId: string;
  // Every mailbox of the account; the tree draws the visible ones.
  mailboxes: readonly Mailbox[];
  currentId: string | undefined;
  // The jump letters show where a keyboard is likely: the desktop layout.
  showLetters: boolean;
  // What the folder items take; none until the account's limits are in.
  folders?: FolderShell | null | undefined;
  onNavigate?: (() => void) | undefined;
}

interface ItemProps extends Omit<MailboxTreeProps, "mailboxes" | "cache" | "folders"> {
  row: FlatRow;
  // The one item Tab lands on: the open mailbox, else the first.
  stopId: string | undefined;
}

const ITEM_SELECTOR = '[role="treeitem"]';
const STOP_SELECTOR = '[role="treeitem"][tabindex="0"]';

// New folder reaches the palette without a key.
const NO_KEYS: readonly Chord[] = [];

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
    <li role="none" className={styles.item}>
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
        data-mailbox-id={mailbox.id}
        onClick={props.onNavigate}
      >
        <span className={styles.name}>{mailbox.name}</span>
        {/* The space keeps the name and the count two words in the link's text. */}
        {count > 0 && (
          <span className={styles.count}>
            {" "}
            <MonoCount value={count} locale={props.locale} tone={current ? "accent" : "muted"} />
          </span>
        )}
      </Link>
      {/* The letter stands outside the link, so the link shows nothing its name leaves out. */}
      {props.showLetters && row.letter !== null && (
        <span className={styles.letter} aria-hidden="true">
          {row.letter}
        </span>
      )}
    </li>
  );
}

// An item that leaves while it holds the focus, as a deleted folder's
// does, hands the focus to the tree's tab stop. A removed element sends
// no event, so the focus is followed through the tree's own.
function useFocusKept(treeRef: RefObject<HTMLUListElement | null>) {
  const heldRef = useRef(false);
  useLayoutEffect(() => {
    if (heldRef.current && focusLost()) {
      treeRef.current?.querySelector<HTMLElement>(STOP_SELECTOR)?.focus({ preventScroll: true });
    }
  });
  return {
    onFocus: () => {
      heldRef.current = true;
    },
    onBlur: (event: FocusEvent<HTMLUListElement>) => {
      heldRef.current = event.currentTarget.contains(event.relatedTarget);
    },
  };
}

type RowRef = (row: HTMLLIElement | null) => (() => void) | undefined;

// Where the caption's row starts in the tree's box. The New folder
// control stands outside the tree, which holds its items alone; it is
// placed on that row, measured when the row comes in and again as the
// tree's rows grow or shrink. The row comes in afresh when the roles
// above it change.
function useCaptionTop(): [number, RowRef] {
  const [top, setTop] = useState(0);
  const place: RowRef = (row) => {
    const tree = row?.parentElement;
    if (row === null || tree === null || tree === undefined) {
      return undefined;
    }
    const measure = (): void => {
      setTop(row.offsetTop);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(tree);
    return () => {
      observer.disconnect();
    };
  };
  return [top, place];
}

interface NewFolderProps {
  locale: Locale;
  top: number;
  onCreate: () => void;
}

// The icon button at the caption's end that asks for a folder at the top.
function NewFolderButton({ locale, top, onCreate }: NewFolderProps) {
  const label = m.command_folder({ kind: "new" }, { locale });
  const button = (
    <button
      type="button"
      className={cx(iconButton.button, styles.newFolder)}
      style={{ insetBlockStart: `${String(top)}px` }}
      aria-label={label}
      onClick={onCreate}
    >
      <FolderPlus className={styles.newFolderIcon} aria-hidden="true" />
    </button>
  );
  return <HintedButton label={label} keys={NO_KEYS} align="end" button={button} />;
}

// The mailboxes of one account as a navigation tree: the roles in their
// fixed order, then the folders with their depth on each row; one tab
// stop with the arrow keys inside. The tree draws the visible mailboxes.
// Each item opens the mailbox's menu by a right click, the context-menu
// key and a long press; the caption's control asks for a new folder.
export function MailboxTree({ mailboxes, cache, folders = null, ...props }: MailboxTreeProps) {
  const { locale, accountId, currentId } = props;
  const treeRef = useRef<HTMLUListElement>(null);
  const menu = useTreeMenu({ locale, cache, accountId, mailboxes, folders, shownId: currentId });
  const kept = useFocusKept(treeRef);
  const [top, placeCaption] = useCaptionTop();
  const visible = visibleMailboxes(mailboxes);
  const model = buildTree(visible, locale);
  const roles = flatten(model.roles);
  const rows = flatten(model.folders);
  const held = visible.some((row) => row.id === currentId);
  const stopId = held ? currentId : (roles[0] ?? rows[0])?.mailbox.id;
  const create =
    folders?.limits.mayCreateTopLevelMailbox === true
      ? () => {
          askNewFolder({ cache, accountId, mailboxes, limits: folders.limits }, null);
        }
      : null;
  return (
    <div className={styles.box}>
      <ul
        ref={treeRef}
        role="tree"
        aria-label={m.mail_mailboxes({}, { locale })}
        className={styles.tree}
        onKeyDown={(event) => {
          if (!menu.onKey(event)) {
            moveFocus(event);
          }
        }}
        onFocus={kept.onFocus}
        onBlur={kept.onBlur}
        onContextMenu={menu.onContextMenu}
        onPointerDown={menu.onPointerDown}
        onPointerUp={menu.onPointerEnd}
        onPointerCancel={menu.onPointerEnd}
        onPointerLeave={menu.onPointerEnd}
        onClickCapture={menu.onClickCapture}
      >
        {roles.map((row) => (
          <TreeItem key={row.mailbox.id} row={row} stopId={stopId} {...props} />
        ))}
        {(rows.length > 0 || create !== null) && (
          <li key={roles.length} ref={placeCaption} role="none" className={styles.caption}>
            <span className={styles.groupLabel} aria-hidden="true">
              {m.tree_folders({}, { locale })}
            </span>
          </li>
        )}
        {rows.map((row) => (
          <TreeItem key={row.mailbox.id} row={row} stopId={stopId} {...props} />
        ))}
      </ul>
      {create !== null && <NewFolderButton locale={locale} top={top} onCreate={create} />}
      {menu.menu}
    </div>
  );
}
