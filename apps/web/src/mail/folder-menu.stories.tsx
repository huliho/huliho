// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";

import { FolderMenu, TreeMenu } from "./folder-menu";
import type { FolderMenuItem } from "./folder-menu";
import header from "./pane-header.module.css";

function nothing(): void {
  // A drawn menu runs nothing.
}

function item(id: string, label: string, danger = false): FolderMenuItem {
  return { id, label, danger, run: nothing };
}

const SELECT = item("select", "Select");
const MARK_READ = item("markRead", "Mark all as read");
const EMPTY_TRASH = item("empty", "Empty Trash", true);
const EMPTY_JUNK = item("empty", "Empty Junk", true);

// Where the open menus stand, under the header's start.
const AT = { x: 16, y: 56 };

const meta: Meta = {
  title: "Mail/FolderMenu",
};

export default meta;

// The mailbox's name in the header as the button of its menu.
export const Trigger: StoryObj = {
  render: () => (
    <header className={header.header}>
      <h1 className={header.title}>
        <FolderMenu name="Trash" items={[MARK_READ, EMPTY_TRASH]} />
      </h1>
    </header>
  ),
};

// Junk's menu: mark all as read, then the empty below a rule in danger.
export const Junk: StoryObj = {
  render: () => (
    <TreeMenu open at={AT} items={[MARK_READ, EMPTY_JUNK]} item={null} onOpenChange={nothing} />
  ),
};

// On touch Select comes first.
export const Touch: StoryObj = {
  render: () => (
    <TreeMenu
      open
      at={AT}
      items={[SELECT, MARK_READ, EMPTY_TRASH]}
      item={null}
      onOpenChange={nothing}
    />
  ),
  globals: { density: "touch" },
};
