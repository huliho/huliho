// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";

import { COMMANDS } from "./fixtures";
import { ShortcutOverlay } from "./shortcut-overlay";

function nothing(): void {
  // Stories render states; the overlay stays open.
}

const meta: Meta = {
  title: "Commands/Shortcut overlay",
};

export default meta;

export const Default: StoryObj = {
  render: () => <ShortcutOverlay open onOpenChange={nothing} locale="en" commands={COMMANDS} />,
};
