// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Tabs as BaseTabs } from "@base-ui/react/tabs";
import type { ReactNode } from "react";

import { cx } from "./cx";
import styles from "./tabs.module.css";

export interface TabItem<Value extends string> {
  value: Value;
  label: string;
}

interface TabsProps<Value extends string> {
  value: Value;
  onValueChange: (value: Value) => void;
  tabs: readonly TabItem<Value>[];
  // One panel per tab; the active one alone stands in the tree.
  panels: Record<Value, ReactNode>;
  // The panels' own class, for a sheet with a border or a box that scrolls.
  panelClassName?: string | undefined;
}

// A row of tabs over one panel. The arrow keys move between the tabs
// and show each one as the focus lands on it; a narrow screen shares
// the row's width among them. The panel is a tab stop of its own.
export function Tabs<Value extends string>(props: TabsProps<Value>) {
  const { value, onValueChange, tabs, panels, panelClassName } = props;
  return (
    <BaseTabs.Root
      value={value}
      onValueChange={(next: unknown) => {
        const picked = tabs.find((tab) => tab.value === next);
        if (picked !== undefined) {
          onValueChange(picked.value);
        }
      }}
    >
      <BaseTabs.List className={styles.list} activateOnFocus>
        {tabs.map((tab) => (
          <BaseTabs.Tab key={tab.value} value={tab.value} className={styles.tab}>
            {tab.label}
          </BaseTabs.Tab>
        ))}
      </BaseTabs.List>
      {tabs.map((tab) => (
        <BaseTabs.Panel
          key={tab.value}
          value={tab.value}
          className={cx(styles.panel, panelClassName)}
        >
          {panels[tab.value]}
        </BaseTabs.Panel>
      ))}
    </BaseTabs.Root>
  );
}
