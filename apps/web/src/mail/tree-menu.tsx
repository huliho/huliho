// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MailCache, Mailbox } from "@huliho/core";
import { Suspense, use, useRef, useState } from "react";
import type { KeyboardEvent, MouseEvent, PointerEvent, ReactNode } from "react";

import type { MenuPoint } from "../design-system/menu";
import type { Locale } from "../paraglide/runtime.js";
import { jobItems } from "./actions/folder-jobs";
import type { FolderItem } from "./actions/folder-jobs";
import { useRunningJob } from "./actions/job-store";
import { useLongPress } from "./actions/long-press";
import type { TreeMenuProps } from "./folder-menu";
import { prefetchMenus } from "./menus-chunk";

const ITEM_SELECTOR = '[role="treeitem"]';

// The menu as it stands open: on which item, for which mailbox, where.
interface Opened {
  item: HTMLElement;
  mailbox: Mailbox;
  at: MenuPoint;
}

interface TreeMenuInput {
  locale: Locale;
  cache: MailCache;
  accountId: string;
  mailboxes: readonly Mailbox[];
}

// What the tree wears to open the menu, and the menu itself.
export interface TreeMenuHandle {
  onContextMenu: (event: MouseEvent<HTMLElement>) => void;
  // True when the key opened the menu.
  onKey: (event: KeyboardEvent<HTMLElement>) => boolean;
  onPointerDown: (event: PointerEvent<HTMLElement>) => void;
  onPointerEnd: () => void;
  onClickCapture: (event: MouseEvent<HTMLElement>) => void;
  menu: ReactNode;
}

function LoadedTreeMenu(props: TreeMenuProps) {
  const { TreeMenu } = use(prefetchMenus());
  return <TreeMenu {...props} />;
}

function itemOf(target: EventTarget | null): HTMLElement | null {
  return target instanceof Element ? target.closest<HTMLElement>(ITEM_SELECTOR) : null;
}

// Below the item's start edge, as the context-menu key has no pointer.
function belowStart(item: HTMLElement): MenuPoint {
  const box = item.getBoundingClientRect();
  const rtl = getComputedStyle(item).direction === "rtl";
  return { x: rtl ? box.right : box.left, y: box.bottom };
}

function isMenuKey(event: KeyboardEvent<HTMLElement>): boolean {
  return event.key === "ContextMenu" || (event.key === "F10" && event.shiftKey);
}

// Where the menu stands open and the call that opens it on an item whose
// mailbox offers something. That call answers whether the item offers
// anything, so an open menu answers yes as well.
function useOpened(input: TreeMenuInput, running: boolean) {
  const { locale, cache, accountId, mailboxes } = input;
  const [opened, setOpened] = useState<Opened | null>(null);
  const [open, setOpen] = useState(false);
  const openRef = useRef(false);
  const itemsOf = (mailbox: Mailbox): FolderItem[] =>
    jobItems({ locale, cache, accountId, mailbox }, running);
  return {
    opened,
    open,
    itemsOf,
    openAt: (item: HTMLElement, at: MenuPoint): boolean => {
      const mailbox = mailboxes.find((row) => row.id === item.dataset["mailboxId"]);
      if (mailbox === undefined || itemsOf(mailbox).length === 0) {
        return false;
      }
      if (!openRef.current) {
        openRef.current = true;
        setOpened({ item, mailbox, at });
        setOpen(true);
      }
      return true;
    },
    onOpenChange: (next: boolean) => {
      openRef.current = next;
      setOpen(next);
    },
  };
}

// The mailbox's menu on a tree item: a right click and a long press open
// it at the pointer, the context-menu key below the item. An item whose
// mailbox offers nothing keeps the browser's own menu.
export function useTreeMenu(input: TreeMenuInput): TreeMenuHandle {
  const running = useRunningJob(input.accountId) !== null;
  const { opened, open, itemsOf, openAt, onOpenChange } = useOpened(input, running);
  const pressed = useRef<{ item: HTMLElement; at: MenuPoint } | null>(null);
  const longPress = useLongPress(
    (target) => (itemOf(target) === null ? null : 0),
    () => {
      if (pressed.current !== null) {
        openAt(pressed.current.item, pressed.current.at);
      }
    },
    true,
  );
  return {
    onContextMenu: (event) => {
      const item = itemOf(event.target);
      if (item !== null && openAt(item, { x: event.clientX, y: event.clientY })) {
        event.preventDefault();
      }
    },
    onKey: (event) => {
      const item = itemOf(event.target);
      if (!isMenuKey(event) || item === null || !openAt(item, belowStart(item))) {
        return false;
      }
      event.preventDefault();
      return true;
    },
    onPointerDown: (event) => {
      const item = itemOf(event.target);
      pressed.current = item === null ? null : { item, at: { x: event.clientX, y: event.clientY } };
      longPress.onPointerDown(event);
    },
    onPointerEnd: longPress.onPointerEnd,
    onClickCapture: (event) => {
      if (longPress.takeClick()) {
        event.preventDefault();
        event.stopPropagation();
      }
    },
    menu:
      opened === null ? null : (
        <Suspense fallback={null}>
          <LoadedTreeMenu
            open={open}
            at={opened.at}
            items={itemsOf(opened.mailbox)}
            item={opened.item}
            onOpenChange={onOpenChange}
          />
        </Suspense>
      ),
  };
}
