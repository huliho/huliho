// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Command } from "./registry";

function nothing(): void {
  // A fixture command runs nothing.
}

// A registry as the mail screen fills it: the list's moves, the jumps
// with their letters and one folder without, the marker's reveal and
// the app's own three.
export const COMMANDS: readonly Command[] = [
  {
    id: "list.next",
    label: "Next conversation",
    group: "navigate",
    keys: [{ key: "j" }],
    run: nothing,
  },
  {
    id: "list.previous",
    label: "Previous conversation",
    group: "navigate",
    keys: [{ key: "k" }],
    run: nothing,
  },
  {
    id: "list.open",
    label: "Open conversation",
    group: "navigate",
    keys: [{ key: "o" }],
    run: nothing,
  },
  {
    id: "thread.close",
    label: "Close conversation",
    group: "navigate",
    keys: [{ key: "Escape" }],
    run: nothing,
  },
  {
    id: "go.inbox",
    label: "Go to Inbox",
    group: "go",
    keys: [{ key: "g" }, { key: "i" }],
    run: nothing,
  },
  {
    id: "go.drafts",
    label: "Go to Drafts",
    group: "go",
    keys: [{ key: "g" }, { key: "d" }],
    run: nothing,
  },
  {
    id: "go.facturen",
    label: "Go to Facturen",
    group: "go",
    keys: [{ key: "g" }, { key: "f" }],
    run: nothing,
  },
  { id: "go.2024", label: "Go to 2024", group: "go", keys: [], run: nothing },
  { id: "list.reveal", label: "Show new mail", group: "act", keys: [{ key: "." }], run: nothing },
  {
    id: "palette.open",
    label: "Command palette",
    group: "app",
    keys: [{ key: "k", mod: true }],
    run: nothing,
  },
  {
    id: "shortcuts.open",
    label: "Keyboard shortcuts",
    group: "app",
    keys: [{ key: "?" }],
    run: nothing,
  },
  {
    id: "account.switch",
    label: "Switch account",
    group: "app",
    keys: [{ key: "l", mod: true, shift: true }],
    run: nothing,
  },
];
