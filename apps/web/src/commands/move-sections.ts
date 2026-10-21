// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { collatorFor, indexIn } from "./palette-sections";

// One mailbox a move can go to: its name and the names of the mailboxes
// above it, outermost first.
export interface MoveTarget {
  id: string;
  name: string;
  path: readonly string[];
}

// What the move mode is handed: the targets in the tree's order, the
// one last moved to on this device and what a pick does.
export interface MoveMode {
  targets: readonly MoveTarget[];
  recent: string | null;
  onPick: (id: string) => void;
}

// One row of the picker. `hit` is where the query stands in the name,
// null while nothing is typed.
export interface MoveEntry {
  value: string;
  label: string;
  target: MoveTarget;
  hit: { start: number; end: number } | null;
}

export interface MoveSection {
  value: string;
  label: string;
  items: MoveEntry[];
}

const RECENT = "recent";
const MAILBOXES = "mailboxes";
// Between a folder and the one above it, as the row draws and reads it.
export const PATH_SEPARATOR = " / ";

function entry(target: MoveTarget, section: string, hit: MoveEntry["hit"]): MoveEntry {
  return {
    value: `${section}:${target.id}`,
    label: [...target.path, target.name].join(PATH_SEPARATOR),
    target,
    hit,
  };
}

// What the picker lists: with nothing typed the last target under
// Recent and every target in the tree's order; else the targets whose
// name holds the query, still in the tree's order.
export function moveSectionsOf(mode: MoveMode, query: string, locale: Locale): MoveSection[] {
  const trimmed = query.trim().normalize("NFC");
  if (trimmed === "") {
    const recent = mode.targets.find((target) => target.id === mode.recent);
    const all = mode.targets.map((target) => entry(target, MAILBOXES, null));
    return [
      ...(recent === undefined
        ? []
        : [
            {
              value: RECENT,
              label: m.palette_recent({}, { locale }),
              items: [entry(recent, RECENT, null)],
            },
          ]),
      { value: MAILBOXES, label: m.move_mailboxes({}, { locale }), items: all },
    ].filter((section) => section.items.length > 0);
  }
  const collator = collatorFor(locale);
  const found = mode.targets.flatMap((target) => {
    const start = indexIn(target.name.normalize("NFC"), trimmed, collator);
    return start === -1 ? [] : [entry(target, MAILBOXES, { start, end: start + trimmed.length })];
  });
  return found.length === 0
    ? []
    : [{ value: MAILBOXES, label: m.move_mailboxes({}, { locale }), items: found }];
}
