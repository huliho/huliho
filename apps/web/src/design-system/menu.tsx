// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Menu } from "@base-ui/react/menu";
import { Check } from "lucide-react";
import type { ReactElement, ReactNode } from "react";

import styles from "./menu.module.css";

// The popup sits this far below its trigger.
const MENU_OFFSET_PX = 4;

export const MenuRoot = Menu.Root;
export const MenuTrigger = Menu.Trigger;
export const MenuRadioGroup = Menu.RadioGroup;

// The popup under its trigger, aligned with the trigger's start edge.
export function MenuPopup({ children }: { children: ReactNode }) {
  return (
    <Menu.Portal>
      <Menu.Positioner side="bottom" align="start" sideOffset={MENU_OFFSET_PX}>
        <Menu.Popup className={styles.popup}>{children}</Menu.Popup>
      </Menu.Positioner>
    </Menu.Portal>
  );
}

interface MenuItemProps {
  onClick: () => void;
  children: ReactNode;
}

export function MenuItem({ onClick, children }: MenuItemProps) {
  return (
    <Menu.Item className={styles.item} onClick={onClick}>
      {children}
    </Menu.Item>
  );
}

interface MenuLinkItemProps {
  // The link the item renders as, so a router link keeps its own navigation.
  render: ReactElement;
  children: ReactNode;
}

export function MenuLinkItem({ render, children }: MenuLinkItemProps) {
  return (
    <Menu.LinkItem className={styles.item} render={render}>
      {children}
    </Menu.LinkItem>
  );
}

interface MenuRadioItemProps {
  value: string;
  children: ReactNode;
}

// One choice of a radio group; the chosen one carries a check mark. A
// choice closes the menu, which Base UI leaves open for a radio item.
export function MenuRadioItem({ value, children }: MenuRadioItemProps) {
  return (
    <Menu.RadioItem value={value} className={styles.item} closeOnClick>
      {children}
      <Menu.RadioItemIndicator className={styles.indicator}>
        <Check className={styles.check} aria-hidden="true" />
      </Menu.RadioItemIndicator>
    </Menu.RadioItem>
  );
}

export function MenuSeparator() {
  return <Menu.Separator className={styles.separator} />;
}
