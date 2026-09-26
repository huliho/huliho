// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { chordOf, sameKeys } from "./keys";
import type { Chord } from "./keys";

// Where a command sits in the palette and the overlay.
export type CommandGroup = "navigate" | "go" | "act" | "app";

export interface Command {
  id: string;
  label: string;
  group: CommandGroup;
  // What runs it from the keyboard: one chord, two for a prefix and its
  // letter, none for a command the palette alone reaches.
  keys: readonly Chord[];
  run: () => void;
}

// A prefix waits this long for its letter.
export const CHORD_TIMEOUT_MS = 1000;

const commands: Command[] = [];
const listeners = new Set<() => void>();
let snapshot: readonly Command[] = [];

// Typing keeps the single keys to itself; a layered surface keeps every key.
const CLAIMED_BY_TYPING = "input, textarea, select, [contenteditable]";
const CLAIMED_BY_LAYER = '[role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"]';

interface Prefix {
  key: string;
  timer: ReturnType<typeof setTimeout>;
}

let prefix: Prefix | null = null;

function changed(): void {
  snapshot = [...commands];
  for (const listener of listeners) {
    listener();
  }
}

// Later registrations win, so a toast's undo outranks a page command.
export function registerCommand(command: Command): () => void {
  commands.push(command);
  changed();
  return () => {
    const index = commands.lastIndexOf(command);
    if (index !== -1) {
      commands.splice(index, 1);
      changed();
    }
  };
}

export function subscribeCommands(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

// The commands as registered, oldest first; a new array per change.
export function commandsSnapshot(): readonly Command[] {
  return snapshot;
}

function claimedBy(target: EventTarget | null, selector: string): boolean {
  return target instanceof Element && target.closest(selector) !== null;
}

function claimed(event: KeyboardEvent, chord: Chord): boolean {
  return (
    claimedBy(event.target, CLAIMED_BY_LAYER) ||
    (chord.mod !== true && claimedBy(event.target, CLAIMED_BY_TYPING))
  );
}

// The pending prefix, taken and cleared.
function takePrefix(): string | null {
  if (prefix === null) {
    return null;
  }
  clearTimeout(prefix.timer);
  const { key } = prefix;
  prefix = null;
  return key;
}

function startsSequence(key: string): boolean {
  return commands.some(
    (command) =>
      command.keys.length > 1 && command.keys[0]?.key === key && command.keys[0].mod !== true,
  );
}

// A key that opens a registered sequence is held as its prefix.
function openPrefix(chord: Chord, event: KeyboardEvent): boolean {
  if (chord.mod === true || !startsSequence(chord.key)) {
    return false;
  }
  event.preventDefault();
  prefix = {
    key: chord.key,
    timer: setTimeout(() => {
      prefix = null;
    }, CHORD_TIMEOUT_MS),
  };
  return true;
}

function runFor(keys: readonly Chord[], event: KeyboardEvent): void {
  const command = commands.findLast((candidate) => sameKeys(candidate.keys, keys));
  if (command === undefined) {
    return;
  }
  event.preventDefault();
  command.run();
}

export function dispatchKey(event: KeyboardEvent): void {
  const chord = event.defaultPrevented || event.altKey ? null : chordOf(event);
  if (chord === null || claimed(event, chord)) {
    return;
  }
  const opening = takePrefix();
  // The key after a prefix completes its sequence or nothing; it never
  // runs on its own. A combination cancels the prefix and runs as usual.
  if (opening !== null && chord.mod !== true) {
    runFor([{ key: opening }, chord], event);
    return;
  }
  if (!openPrefix(chord, event)) {
    runFor([chord], event);
  }
}

export function installCommandListener(target: Window = window): () => void {
  target.addEventListener("keydown", dispatchKey);
  return () => {
    takePrefix();
    target.removeEventListener("keydown", dispatchKey);
  };
}
