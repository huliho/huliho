// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";

import { CommandPalette } from "./command-palette";
import { COMMANDS } from "./fixtures";

function nothing(): void {
  // Stories render states; the palette stays open and runs nothing.
}

function Palette({ query, recent }: { query?: string; recent?: string[] }) {
  return (
    <CommandPalette
      open
      onOpenChange={nothing}
      locale="en"
      commands={COMMANDS}
      recent={recent ?? []}
      defaultQuery={query}
      onRun={nothing}
    />
  );
}

const meta: Meta = {
  title: "Commands/Palette",
};

export default meta;

// Every command by group, the ones last run first.
export const Default: StoryObj = {
  render: () => <Palette recent={["go.drafts", "shortcuts.open"]} />,
};

export const Narrowed: StoryObj = {
  render: () => <Palette query="go" />,
};

export const NoMatch: StoryObj = {
  render: () => <Palette query="zzz" />,
};
