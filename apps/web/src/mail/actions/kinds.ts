// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MailAction } from "@huliho/core";

import type { Chord } from "../../commands/keys";
import type { Locale } from "../../paraglide/runtime.js";
import { takenBy } from "./taken";
import type { Aim } from "./taken";
import { actionLabel } from "./words";

// What a key asks of a thread; the mailbox and the thread decide which
// action that is.
export type ActionKind = "archive" | "delete" | "spam" | "read" | "star" | "move";

// Each command's id and keys, in the keyboard map's order; `#` and `!`
// match the key the layout types, whatever it takes to type them.
export const ACTION_COMMANDS: ReadonlyMap<ActionKind, { id: string; keys: readonly Chord[] }> =
  new Map([
    ["archive", { id: "thread.archive", keys: [{ key: "e" }] }],
    ["delete", { id: "thread.delete", keys: [{ key: "#" }] }],
    ["spam", { id: "thread.spam", keys: [{ key: "!" }] }],
    ["read", { id: "thread.read", keys: [{ key: "u" }] }],
    ["star", { id: "thread.star", keys: [{ key: "s" }] }],
    ["move", { id: "thread.move", keys: [{ key: "m" }] }],
  ]);

// The ones the bulk bar holds, in the same order; star stays with its key.
export const BULK_KINDS: readonly ActionKind[] = ["archive", "delete", "spam", "read", "move"];

// One action a surface draws for its threads: the action it takes there,
// its label and the keys of its command.
export interface ShownAction {
  kind: ActionKind;
  id: string;
  action: MailAction;
  label: string;
  keys: readonly Chord[];
}

// The actions of `kinds` a surface draws for its threads, each while a
// press would change one of them from the mailbox it acts from.
export function shownActions(
  kinds: readonly ActionKind[],
  aims: readonly Aim[],
  locale: Locale,
): ShownAction[] {
  return kinds.flatMap((kind) => {
    const command = ACTION_COMMANDS.get(kind);
    const taken = takenBy(kind, aims);
    if (command === undefined || taken === null || taken.taken.length === 0) {
      return [];
    }
    const { action } = taken;
    return [
      { kind, id: command.id, action, label: actionLabel(action, locale), keys: command.keys },
    ];
  });
}
