// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { actionFor } from "@huliho/core";
import type { ActionTarget, MailAction, Mailbox } from "@huliho/core";

import type { Chord } from "../../commands/keys";
import { useRegistered } from "../../commands/use-command";
import type { Locale } from "../../paraglide/runtime.js";
import { actionLabel } from "./words";

// What a key asks of a thread; the mailbox and the thread decide which
// action that is.
export type ActionKind = "archive" | "delete" | "spam" | "read" | "star";

// Each command's id and keys, in the keyboard map's order; `#` and `!`
// match the key the layout types, whatever it takes to type them.
export const ACTION_COMMANDS: ReadonlyMap<ActionKind, { id: string; keys: readonly Chord[] }> =
  new Map([
    ["archive", { id: "thread.archive", keys: [{ key: "e" }] }],
    ["delete", { id: "thread.delete", keys: [{ key: "#" }] }],
    ["spam", { id: "thread.spam", keys: [{ key: "!" }] }],
    ["read", { id: "thread.read", keys: [{ key: "u" }] }],
    ["star", { id: "thread.star", keys: [{ key: "s" }] }],
  ]);

// The ones the bulk bar holds, in the same order; star stays with its key.
export const BULK_KINDS: readonly ActionKind[] = ["archive", "delete", "spam", "read"];

// One action a surface draws for a thread: the action it takes there,
// its label and the keys of its command.
export interface ShownAction {
  kind: ActionKind;
  id: string;
  action: MailAction;
  label: string;
  keys: readonly Chord[];
}

// The actions of `kinds` a surface draws for one thread, each while the
// registry holds its command, so a button stands exactly while its key
// works.
export function useShownActions(
  kinds: readonly ActionKind[],
  mailbox: Mailbox,
  thread: Pick<ActionTarget, "unread" | "flagged">,
  locale: Locale,
): ShownAction[] {
  const commands = useRegistered();
  return kinds.flatMap((kind) => {
    const id = ACTION_COMMANDS.get(kind)?.id;
    const command = commands.findLast((entry) => entry.id === id);
    if (command === undefined) {
      return [];
    }
    const action = actionFor(kind, mailbox, thread);
    return [
      { kind, id: command.id, action, label: actionLabel(action, locale), keys: command.keys },
    ];
  });
}
