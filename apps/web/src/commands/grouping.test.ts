// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test } from "vitest";

import { COMMANDS } from "./fixtures";
import { groupedCommands } from "./grouping";
import type { Command } from "./registry";

function nothing(): void {
  // A test command runs nothing.
}

function act(id: string, key: string): Command {
  return { id, label: id, group: "act", keys: [{ key }], run: nothing };
}

test("the Act group lists the thread actions in the keyboard map's order, ahead of undo and the marker", () => {
  // The fixtures hold the marker's reveal; the rest registers out of order.
  const registered: Command[] = [
    ...COMMANDS,
    act("undo", "z"),
    act("thread.star", "s"),
    act("thread.read", "u"),
    act("thread.spam", "!"),
    act("thread.delete", "#"),
    act("thread.archive", "e"),
  ];
  const group = groupedCommands(registered, "en").find((grouping) => grouping.group === "act");
  expect(group?.commands.map((command) => command.id)).toEqual([
    "thread.archive",
    "thread.delete",
    "thread.spam",
    "thread.read",
    "thread.star",
    "undo",
    "list.reveal",
  ]);
});
