// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// One key press: the key as the event names it, with the platform's
// command modifier and Shift where a combination asks for them.
export interface Chord {
  key: string;
  // Cmd on an Apple platform, Ctrl everywhere else.
  mod?: boolean;
  shift?: boolean;
}

export const ESCAPE: Chord = { key: "Escape" };

const APPLE = /Mac|iPhone|iPad|iPod/u;
// The bare modifiers, which never form a chord of their own.
const MODIFIERS = new Set(["Shift", "Control", "Meta", "Alt", "AltGraph", "CapsLock"]);
// What a key cap shows for a key the event names in words.
const LEGENDS = new Map([
  ["Escape", "esc"],
  ["Enter", "↵"],
  ["ArrowUp", "↑"],
  ["ArrowDown", "↓"],
]);

function isApple(): boolean {
  return APPLE.test(navigator.userAgent);
}

// The chord a key event carries: null for a bare modifier and for the
// other platform's command key, which is not ours to answer.
export function chordOf(event: KeyboardEvent): Chord | null {
  if (MODIFIERS.has(event.key)) {
    return null;
  }
  const apple = isApple();
  const mod = apple ? event.metaKey : event.ctrlKey;
  const other = apple ? event.ctrlKey : event.metaKey;
  if (other) {
    return null;
  }
  return mod
    ? { key: event.key.toLowerCase(), mod: true, shift: event.shiftKey }
    : { key: event.key };
}

// A plain key matches by its own name, so `?` is `?` whatever Shift
// did; under the modifier the letter matches whatever its case.
function sameChord(first: Chord, second: Chord): boolean {
  if ((first.mod ?? false) !== (second.mod ?? false)) {
    return false;
  }
  if (first.mod !== true) {
    return first.key === second.key;
  }
  return (
    first.key.toLowerCase() === second.key.toLowerCase() &&
    (first.shift ?? false) === (second.shift ?? false)
  );
}

export function sameKeys(first: readonly Chord[], second: readonly Chord[]): boolean {
  return (
    first.length === second.length &&
    first.every((chord, index) => {
      const other = second.at(index);
      return other !== undefined && sameChord(chord, other);
    })
  );
}

function legend(key: string, combined: boolean): string {
  const named = LEGENDS.get(key);
  if (named !== undefined) {
    return named;
  }
  return combined ? key.toUpperCase() : key;
}

// What a cap shows for one chord: the modifiers as the platform draws
// them, then the key.
export function chordText(chord: Chord): string {
  const apple = isApple();
  const parts: string[] = [];
  if (chord.mod === true) {
    parts.push(apple ? "⌘" : "Ctrl");
  }
  if (chord.shift === true) {
    parts.push(apple ? "⇧" : "Shift");
  }
  parts.push(legend(chord.key, chord.mod === true));
  return parts.join(apple ? "" : "+");
}

// A sequence reads as its chords with a space between them.
export function keysText(keys: readonly Chord[]): string {
  return keys.map((chord) => chordText(chord)).join(" ");
}
