// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { currentCommands, groupedCommands } from "./grouping";
import { keysText } from "./keys";
import type { Command } from "./registry";

// One row of the palette: its value tells two rows of one command
// apart and its label is what the list reads for the row.
export interface Entry {
  value: string;
  label: string;
  command: Command;
}

export interface Section {
  value: string;
  label: string;
  items: Entry[];
}

const RECENT = "recent";

// How well a command answers a query, best first: its label starts with
// it, a word of the label does, the label holds it, its keys do.
type Tier = 0 | 1 | 2 | 3;

interface Ranked {
  entry: Entry;
  tier: Tier;
}

// Letters compare by their base, so a typed "e" finds an "é"; punctuation
// counts, so a typed "?" finds the key and not every space.
function collatorFor(locale: Locale): Intl.Collator {
  return new Intl.Collator(locale, { usage: "search", sensitivity: "base" });
}

function startsWith(text: string, query: string, collator: Intl.Collator): boolean {
  return text.length >= query.length && collator.compare(text.slice(0, query.length), query) === 0;
}

function contains(text: string, query: string, collator: Intl.Collator): boolean {
  for (let start = 0; start <= text.length - query.length; start += 1) {
    if (collator.compare(text.slice(start, start + query.length), query) === 0) {
      return true;
    }
  }
  return false;
}

// The slices go by code units, so both sides compare in composed form.
function tierOf(command: Command, query: string, collator: Intl.Collator): Tier | null {
  const label = command.label.normalize("NFC");
  if (startsWith(label, query, collator)) {
    return 0;
  }
  if (label.split(" ").some((word) => startsWith(word, query, collator))) {
    return 1;
  }
  if (contains(label, query, collator)) {
    return 2;
  }
  const keys = keysText(command.keys).replaceAll(" ", "").normalize("NFC");
  return contains(keys, query.replaceAll(" ", ""), collator) ? 3 : null;
}

function entry(command: Command, section: string): Entry {
  return { value: `${section}:${command.id}`, label: command.label, command };
}

// Every command in its group, the ones last run from the palette first.
function listed(
  commands: readonly Command[],
  recent: readonly string[],
  locale: Locale,
): Section[] {
  const current = currentCommands(commands);
  const recents = recent
    .map((id) => current.find((command) => command.id === id))
    .filter((command) => command !== undefined)
    .map((command) => entry(command, RECENT));
  const sections = groupedCommands(commands, locale).map((grouping) => ({
    value: grouping.group,
    label: grouping.label,
    items: grouping.commands.map((command) => entry(command, grouping.group)),
  }));
  if (recents.length === 0) {
    return sections;
  }
  return [{ value: RECENT, label: m.palette_recent({}, { locale }), items: recents }, ...sections];
}

// The commands that answer the query, best first inside each group.
function matched(commands: readonly Command[], query: string, locale: Locale): Section[] {
  const collator = collatorFor(locale);
  const composed = query.normalize("NFC");
  return groupedCommands(commands, locale)
    .map((grouping) => {
      const ranked: Ranked[] = [];
      for (const command of grouping.commands) {
        const tier = tierOf(command, composed, collator);
        if (tier !== null) {
          ranked.push({ entry: entry(command, grouping.group), tier });
        }
      }
      ranked.sort((first, second) => first.tier - second.tier);
      return {
        value: grouping.group,
        label: grouping.label,
        items: ranked.map((row) => row.entry),
      };
    })
    .filter((section) => section.items.length > 0);
}

// What the palette lists for a query: everything with the recent ones
// first while the query is empty, else the matches by how well they fit.
export function sectionsOf(
  commands: readonly Command[],
  recent: readonly string[],
  query: string,
  locale: Locale,
): Section[] {
  const trimmed = query.trim();
  return trimmed === "" ? listed(commands, recent, locale) : matched(commands, trimmed, locale);
}
