// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";

import type { MoveTarget } from "../../commands/move-sections";
import { forgetStored } from "../forget-stored";
import { buildTree, flatten } from "../tree";

// One key per account holds the mailbox last moved to on this device.
const TARGET_KEY_PREFIX = "huliho-move-target:";

// Which threads a move the picker carries out takes: those the focus or
// the picks name once the palette has given the focus back, the open
// thread for the toolbar's button, the picks for the bulk bar's.
export type MoveSource = "focus" | "open" | "picks";

type MoveRun = (to: string, source: MoveSource) => void;

let lent: MoveRun | null = null;

// What the list lends the picker: the move of the threads a source
// names, as its latest render sees them. A later lender wins, and an
// earlier one taking back leaves the later one standing.
export function lendMove(run: MoveRun): () => void {
  lent = run;
  return () => {
    if (lent === run) {
      lent = null;
    }
  };
}

// Moves to `to` what `source` names; nothing while no list lends.
export function moveLent(to: string, source: MoveSource): void {
  lent?.(to, source);
}

// The names of the mailboxes above one, outermost first. A parent the
// account lacks ends the path, and so does one already named, so a loop
// in the server's parents cannot hold the picker up.
function pathOf(mailbox: Mailbox, byId: ReadonlyMap<string, Mailbox>): string[] {
  const names: string[] = [];
  const seen = new Set([mailbox.id]);
  let parent = mailbox.parentId === null ? undefined : byId.get(mailbox.parentId);
  while (parent !== undefined && !seen.has(parent.id)) {
    seen.add(parent.id);
    names.unshift(parent.name);
    parent = parent.parentId === null ? undefined : byId.get(parent.parentId);
  }
  return names;
}

// The mailboxes a move may enter in the tree's order, the roles first,
// each named with the mailboxes above it.
export function moveTargetsOf(
  ids: readonly string[],
  mailboxes: readonly Mailbox[],
  locale: string,
): MoveTarget[] {
  const allowed = new Set(ids);
  const byId = new Map(mailboxes.map((row) => [row.id, row]));
  const model = buildTree(mailboxes, locale);
  return [...flatten(model.roles), ...flatten(model.folders)]
    .filter((row) => allowed.has(row.mailbox.id))
    .map(({ mailbox }) => ({ id: mailbox.id, name: mailbox.name, path: pathOf(mailbox, byId) }));
}

// The mailbox last moved to from this account on this device; null when
// none is known or the storage refuses to be read.
export function rememberedTarget(accountId: string): string | null {
  try {
    return localStorage.getItem(`${TARGET_KEY_PREFIX}${accountId}`);
  } catch {
    return null;
  }
}

export function rememberTarget(accountId: string, mailboxId: string): void {
  try {
    localStorage.setItem(`${TARGET_KEY_PREFIX}${accountId}`, mailboxId);
  } catch {
    // A device that keeps nothing simply starts without a recent target.
  }
}

// A session that ends in this browser takes every account's memory with it.
export function forgetMoveTargets(): void {
  forgetStored(TARGET_KEY_PREFIX);
}
