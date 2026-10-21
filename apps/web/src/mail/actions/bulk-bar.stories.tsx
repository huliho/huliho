// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MailAction } from "@huliho/core";
import type { Meta, StoryObj } from "@storybook/react-vite";
import type { ReactNode } from "react";

import { ESCAPE } from "../../commands/keys";
import { iconOf } from "./action-icon";
import { BulkBar } from "./bulk-bar";
import type { BulkBarProps, BulkEntry } from "./bulk-handle";
import { ACTION_COMMANDS } from "./kinds";
import type { ActionKind } from "./kinds";

// The list's ground under the pill, with room for its ring.
function Ground({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        display: "flex",
        justifyContent: "center",
        padding: "var(--hhx-space-4)",
        background: "var(--hh-surface)",
      }}
    >
      {children}
    </div>
  );
}

function entry(kind: ActionKind, action: MailAction, label: string): BulkEntry {
  return { kind, label, keys: ACTION_COMMANDS.get(kind)?.keys ?? [], Icon: iconOf(action) };
}

const ARCHIVE = entry("archive", "archive", "Archive");
const DELETE = entry("delete", "delete", "Delete");
const DELETE_FOREVER = entry("delete", "deleteForever", "Delete forever");
const SPAM = entry("spam", "spam", "Mark as spam");
const READ = entry("read", "read", "Mark as read");
const UNREAD = entry("read", "unread", "Mark as unread");
const MOVE = entry("move", "move", "Move to…");

function nothing(): void {
  // A drawn bar acts on nothing.
}

function bar(
  count: number,
  inline: readonly BulkEntry[],
  behind: readonly BulkEntry[] = [],
): StoryObj {
  const props: BulkBarProps = {
    count: { text: `${String(count)} selected`, figure: String(count) },
    inline,
    behind,
    moreLabel: "More actions",
    close: { label: "Close selection", keys: [ESCAPE] },
    run: nothing,
    onClose: nothing,
  };
  return {
    render: () => (
      <Ground>
        <BulkBar {...props} />
      </Ground>
    ),
  };
}

const meta: Meta = {
  title: "Mail/BulkBar",
};

export default meta;

// The inbox at the desktop: every action the picks can take, then Close.
export const Inbox = bar(2, [ARCHIVE, DELETE, SPAM, READ, MOVE]);

// Trash: delete forever in place of delete and no archive.
export const Trash = bar(3, [DELETE_FOREVER, SPAM, UNREAD, MOVE]);

// Under touch: archive, delete and the read flip, with spam and move behind More actions.
export const Touch: StoryObj = {
  ...bar(2, [ARCHIVE, DELETE, READ], [SPAM, MOVE]),
  globals: { density: "touch" },
};

// Select pressed before any pick: Close alone.
export const NothingPicked = bar(0, []);
