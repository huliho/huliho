// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";

import { CommandPalette } from "./command-palette";
import { COMMANDS } from "./fixtures";
import type { MoveMode } from "./move-sections";

function nothing(): void {
  // Stories render states; the palette stays open and runs nothing.
}

// The mailboxes a move from the inbox may enter, Facturen last moved to.
const MOVE: MoveMode = {
  targets: [
    { id: "mb-drafts", name: "Drafts", path: [] },
    { id: "mb-sent", name: "Sent", path: [] },
    { id: "mb-archive", name: "Archive", path: [] },
    { id: "mb-facturen", name: "Facturen", path: [] },
    { id: "mb-verbouwing", name: "Verbouwing", path: [] },
    { id: "mb-offertes", name: "Offertes", path: ["Verbouwing"] },
  ],
  recent: "mb-facturen",
  onPick: nothing,
};

interface PaletteProps {
  query?: string;
  recent?: string[];
  move?: MoveMode;
}

function Palette({ query, recent, move }: PaletteProps) {
  return (
    <CommandPalette
      open
      onOpenChange={nothing}
      locale="en"
      commands={COMMANDS}
      recent={recent ?? []}
      defaultQuery={query}
      onRun={nothing}
      move={move}
      onPick={nothing}
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

// The move mode: the last target under Recent, the tree below it.
export const Move: StoryObj = {
  render: () => <Palette move={MOVE} />,
};

// A query keeps the names that hold it in the tree's order.
export const MoveNarrowed: StoryObj = {
  render: () => <Palette move={MOVE} query="ver" />,
};

export const MoveNoMatch: StoryObj = {
  render: () => <Palette move={MOVE} query="zzz" />,
};
