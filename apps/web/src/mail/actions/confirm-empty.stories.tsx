// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";

import { ConfirmDialog } from "./folder-dialogs";
import { DIALOG_PARTS } from "./folder-host";
import { boxOf } from "./writable-rig";

function nothing(): void {
  // A drawn dialog empties nothing.
}

const meta: Meta = {
  title: "Mail/ConfirmEmpty",
};

export default meta;

// Trash with its twelve messages, Cancel holding the focus.
export const Trash: StoryObj = {
  render: () => (
    <ConfirmDialog
      locale="en"
      ask={{ kind: "empty", mailbox: boxOf("trash"), confirm: nothing }}
      open
      parts={DIALOG_PARTS}
    />
  ),
};

// Junk with one message, worded for one.
export const JunkOfOne: StoryObj = {
  render: () => (
    <ConfirmDialog
      locale="en"
      ask={{ kind: "empty", mailbox: { ...boxOf("junk"), totalEmails: 1 }, confirm: nothing }}
      open
      parts={DIALOG_PARTS}
    />
  ),
};
