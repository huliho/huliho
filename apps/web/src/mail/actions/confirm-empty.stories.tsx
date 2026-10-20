// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";

import { EmptyDialog } from "./confirm-empty";
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
    <EmptyDialog locale="en" mailbox={boxOf("trash")} open onCancel={nothing} onConfirm={nothing} />
  ),
};

// Junk with one message, worded for one.
export const JunkOfOne: StoryObj = {
  render: () => (
    <EmptyDialog
      locale="en"
      mailbox={{ ...boxOf("junk"), totalEmails: 1 }}
      open
      onCancel={nothing}
      onConfirm={nothing}
    />
  ),
};
