// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import type { Command, CommandGroup } from "./registry";

export interface CommandGrouping {
  group: CommandGroup;
  label: string;
  commands: Command[];
}

// The groups in the order the surfaces draw them.
const GROUP_ORDER: readonly CommandGroup[] = ["navigate", "go", "act", "app"];

const GROUP_LABELS = new Map<CommandGroup, (locale: Locale) => string>([
  ["navigate", (locale) => m.commands_group_navigate({}, { locale })],
  ["go", (locale) => m.commands_group_go({}, { locale })],
  ["act", (locale) => m.commands_group_act({}, { locale })],
  ["app", (locale) => m.commands_group_app({}, { locale })],
]);

function groupLabel(group: CommandGroup, locale: Locale): string {
  return GROUP_LABELS.get(group)?.(locale) ?? group;
}

// Where a command sits inside its group, as the keyboard map lists them.
// A command outside this list keeps its place among the others outside
// it, as registered; the jumps come as one batch in the tree's order.
const RANKS = new Map(
  [
    "list.next",
    "list.previous",
    "list.open",
    "thread.close",
    "undo",
    "list.reveal",
    "palette.open",
    "shortcuts.open",
    "account.switch",
    "settings.close",
  ].map((id, index): [string, number] => [id, index]),
);

function rankOf(command: Command): number {
  return RANKS.get(command.id) ?? RANKS.size;
}

function byRank(first: Command, second: Command): number {
  return rankOf(first) - rankOf(second);
}

// One command per id, the last registration of it, since that one
// answers the key.
export function currentCommands(commands: readonly Command[]): Command[] {
  const byId = new Map<string, Command>();
  for (const command of commands) {
    byId.delete(command.id);
    byId.set(command.id, command);
  }
  return [...byId.values()];
}

// The commands by group, in the keyboard map's order inside each group,
// empty groups left out.
export function groupedCommands(commands: readonly Command[], locale: Locale): CommandGrouping[] {
  const current = currentCommands(commands);
  return GROUP_ORDER.map((group) => ({
    group,
    label: groupLabel(group, locale),
    commands: current.filter((command) => command.group === group).toSorted(byRank),
  })).filter((grouping) => grouping.commands.length > 0);
}
