// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { ChevronDown } from "lucide-react";

import { MenuItem, MenuPopup, MenuRoot, MenuTrigger } from "../design-system/menu";
import styles from "./folder-menu.module.css";

// The shell words the menu, so its chunk shares no module with the
// page's start.
export interface FolderMenuProps {
  // The mailbox's name, which the button shows.
  name: string;
  select: { label: string; run: () => void };
}

// The mailbox's name as the button of its menu. On touch the menu holds
// Select, the route to a selection beside the long press.
export function FolderMenu({ name, select }: FolderMenuProps) {
  return (
    <MenuRoot>
      <MenuTrigger className={styles.trigger}>
        <span className={styles.name}>{name}</span>
        <ChevronDown className={styles.chevron} aria-hidden="true" />
      </MenuTrigger>
      <MenuPopup>
        <MenuItem onClick={select.run}>{select.label}</MenuItem>
      </MenuPopup>
    </MenuRoot>
  );
}
