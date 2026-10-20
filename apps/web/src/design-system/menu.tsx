// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Menu } from "@base-ui/react/menu";
import { Check } from "lucide-react";
import type { ReactElement, ReactNode } from "react";

import { cx } from "./cx";
import styles from "./menu.module.css";

// The popup sits this far below its trigger.
const MENU_OFFSET_PX = 4;

export const MenuRoot = Menu.Root;
export const MenuTrigger = Menu.Trigger;
export const MenuRadioGroup = Menu.RadioGroup;

// A point on the screen a menu opens at, as a context menu does.
export interface MenuPoint {
  x: number;
  y: number;
}

interface MenuPopupProps {
  children: ReactNode;
  // Where a menu without a trigger opens; its trigger otherwise.
  at?: MenuPoint | undefined;
  // Where the focus goes once a menu without a trigger closes.
  finalFocus?: HTMLElement | null | undefined;
  // The menu takes the sidebar's width.
  wide?: boolean;
}

function anchorAt({ x, y }: MenuPoint) {
  return { getBoundingClientRect: () => DOMRect.fromRect({ x, y, width: 0, height: 0 }) };
}

// The popup under its trigger or at a point, aligned with the start edge.
export function MenuPopup({ children, at, finalFocus, wide = false }: MenuPopupProps) {
  return (
    <Menu.Portal>
      <Menu.Positioner
        side="bottom"
        align="start"
        sideOffset={at === undefined ? MENU_OFFSET_PX : 0}
        anchor={at === undefined ? undefined : anchorAt(at)}
      >
        <Menu.Popup
          className={cx(styles.popup, wide ? styles.wide : undefined)}
          finalFocus={finalFocus === undefined ? undefined : () => finalFocus}
          onContextMenu={(event) => {
            event.preventDefault();
          }}
        >
          {children}
        </Menu.Popup>
      </Menu.Positioner>
    </Menu.Portal>
  );
}

interface MenuItemProps {
  onClick: () => void;
  // An item that destroys, in the danger color below a rule.
  danger?: boolean;
  children: ReactNode;
}

export function MenuItem({ onClick, danger = false, children }: MenuItemProps) {
  return (
    <Menu.Item className={styles.item} data-danger={danger || undefined} onClick={onClick}>
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
