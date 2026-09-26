// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";
import { useNavigate } from "@tanstack/react-router";

import type { Chord } from "../commands/keys";
import type { Command } from "../commands/registry";
import { useCommands } from "../commands/use-command";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { markedForList } from "./thread-history";
import { buildTree, flatten } from "./tree";

// The key that opens a jump; the mailbox's letter follows it.
const JUMP_PREFIX: Chord = { key: "g" };
const NONE: readonly Command[] = [];

// Where a jump takes the keyboard: the mailbox, with the focus into its list.
type Jump = (mailboxId: string) => void;

// One command per mailbox, in the tree's order: the six roles with
// their fixed letters, then the folders with the letter the tree
// shows; a folder without one is reached through the palette alone.
export function jumpCommands(mailboxes: readonly Mailbox[], locale: Locale, jump: Jump): Command[] {
  const model = buildTree(mailboxes, locale);
  return [...flatten(model.roles), ...flatten(model.folders)].map((row) => ({
    id: `go.${row.mailbox.id}`,
    label: m.command_go_to({ mailbox: row.mailbox.name }, { locale }),
    group: "go",
    keys: row.letter === null ? [] : [JUMP_PREFIX, { key: row.letter }],
    run: () => {
      jump(row.mailbox.id);
    },
  }));
}

interface Shown {
  accountId: string;
  // The mailbox open now; a jump to it replaces the entry rather than adding one.
  mailboxId: string | undefined;
  locale: Locale;
}

// Registers the jumps of the account's tree while the shell stands.
export function useJumpCommands(mailboxes: readonly Mailbox[] | null, shown: Shown): void {
  const navigate = useNavigate();
  const jump: Jump = (mailboxId) => {
    void navigate({
      to: "/mail/$accountId/$mailboxId",
      params: { accountId: shown.accountId, mailboxId },
      replace: mailboxId === shown.mailboxId,
      state: markedForList,
    });
  };
  const commands = mailboxes === null ? NONE : jumpCommands(mailboxes, shown.locale, jump);
  useCommands(commands);
}
