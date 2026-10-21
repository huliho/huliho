// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { ChevronDown } from "lucide-react";
import { Fragment, useRef } from "react";

import { MenuItem, MenuPopup, MenuRoot, MenuSeparator, MenuTrigger } from "../design-system/menu";
import type { MenuPoint } from "../design-system/menu";
import styles from "./folder-menu.module.css";

// One item as the shell words it, so the chunk shares no module with the
// page's start.
export interface FolderMenuItem {
  id: string;
  label: string;
  danger: boolean;
  run: () => void;
}

interface ItemsProps {
  items: readonly FolderMenuItem[];
  // Takes the run of the item pressed, which waits for the menu to close.
  onChoose: (run: () => void) => void;
}

// The items with the ones that destroy below a rule.
function Items({ items, onChoose }: ItemsProps) {
  const firstDanger = items.findIndex((item) => item.danger);
  return items.map((item, index) => (
    <Fragment key={item.id}>
      {index === firstDanger && index > 0 && <MenuSeparator />}
      <MenuItem
        danger={item.danger}
        onClick={() => {
          onChoose(item.run);
        }}
      >
        {item.label}
      </MenuItem>
    </Fragment>
  ));
}

// A press runs once the menu has closed and given the focus back, so a
// dialog it opens hands the focus back there too.
function useChosen() {
  const pending = useRef<(() => void) | null>(null);
  return {
    onChoose: (run: () => void) => {
      pending.current = run;
    },
    onOpenChangeComplete: (open: boolean) => {
      const run = pending.current;
      if (!open && run !== null) {
        pending.current = null;
        run();
      }
    },
  };
}

export interface FolderMenuProps {
  // The mailbox's name, which the button shows.
  name: string;
  items: readonly FolderMenuItem[];
}

// The mailbox's name as the button of its menu.
export function FolderMenu({ name, items }: FolderMenuProps) {
  const chosen = useChosen();
  return (
    <MenuRoot onOpenChangeComplete={chosen.onOpenChangeComplete}>
      <MenuTrigger className={styles.trigger}>
        <span className={styles.name}>{name}</span>
        <ChevronDown className={styles.chevron} aria-hidden="true" />
      </MenuTrigger>
      <MenuPopup wide>
        <Items items={items} onChoose={chosen.onChoose} />
      </MenuPopup>
    </MenuRoot>
  );
}

export interface TreeMenuProps {
  open: boolean;
  // Where it opens: at the pointer for a right click or a long press,
  // below the item's start for the context-menu key.
  at: MenuPoint;
  items: readonly FolderMenuItem[];
  // The tree item it opened on, which takes the focus back.
  item: HTMLElement | null;
  onOpenChange: (open: boolean) => void;
}

// The same menu on a tree item, opened without a trigger of its own.
export function TreeMenu({ open, at, items, item, onOpenChange }: TreeMenuProps) {
  const chosen = useChosen();
  return (
    <MenuRoot
      open={open}
      onOpenChange={onOpenChange}
      onOpenChangeComplete={chosen.onOpenChangeComplete}
    >
      <MenuPopup wide at={at} finalFocus={item}>
        <Items items={items} onChoose={chosen.onChoose} />
      </MenuPopup>
    </MenuRoot>
  );
}
