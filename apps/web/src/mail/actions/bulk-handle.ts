// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ActionTarget, Mailbox } from "@huliho/core";
import type { LucideIcon } from "lucide-react";

import type { Chord } from "../../commands/keys";
import type { ActionKind } from "./kinds";

// What the list hands the bulk bar while a selection stands.
export interface BulkHandle {
  // The mailbox the words follow.
  mailbox: Mailbox;
  count: number;
  // The picks as one thread, whose state the read flip's words follow.
  thread: Pick<ActionTarget, "unread" | "flagged">;
  run: (kind: ActionKind) => void;
  close: () => void;
}

// One button of the bar as the shell draws it for the bar's chunk.
export interface BulkEntry {
  kind: ActionKind;
  label: string;
  keys: readonly Chord[];
  Icon: LucideIcon;
}

// Everything the bar's chunk draws, worked out in the shell, so the
// chunk shares no module with the page's start.
export interface BulkBarProps {
  // The count's sentence and the figure inside it.
  count: { text: string; figure: string };
  inline: readonly BulkEntry[];
  // The actions behind More actions, which touch keeps out of the bar.
  behind: readonly BulkEntry[];
  moreLabel: string;
  close: { label: string; keys: readonly Chord[] };
  run: (kind: ActionKind) => void;
  onClose: () => void;
}
