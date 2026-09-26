// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { keysText } from "../commands/keys";
import type { Command } from "../commands/registry";
import { useRegistered } from "../commands/use-command";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { useLayout } from "../shell/breakpoints";
import styles from "./key-hints.module.css";

interface Hint {
  // The commands the hint stands for; it shows while every one is registered.
  ids: readonly string[];
  label: (locale: Locale) => string;
}

const HINTS: readonly Hint[] = [
  { ids: ["list.next", "list.previous"], label: (locale) => m.hint_move({}, { locale }) },
  { ids: ["list.open"], label: (locale) => m.hint_open({}, { locale }) },
  { ids: ["undo"], label: (locale) => m.hint_undo({}, { locale }) },
  { ids: ["shortcuts.open"], label: (locale) => m.hint_shortcuts({}, { locale }) },
];

interface KeyHintsProps {
  locale: Locale;
  // Whether the foot is taken, by the first-sync block while it shows.
  hidden: boolean;
}

// The keys of a hint, as the registry holds them now; null while one is missing.
function keysOf(hint: Hint, commands: readonly Command[]): string | null {
  const found = hint.ids.map((id) => commands.findLast((command) => command.id === id));
  if (found.some((command) => command === undefined)) {
    return null;
  }
  return found.map((command) => (command === undefined ? "" : keysText(command.keys))).join("/");
}

// The strip at the foot of the list: a few keys with a word each, read
// from the registry, where a keyboard is likely.
export function KeyHints({ locale, hidden }: KeyHintsProps) {
  const layout = useLayout();
  const commands = useRegistered();
  if (hidden || layout === "phone") {
    return null;
  }
  const hints = HINTS.map((hint) => ({ keys: keysOf(hint, commands), label: hint.label(locale) }));
  return (
    <p className={styles.strip}>
      {hints.map(
        (hint) =>
          hint.keys !== null && (
            <span key={hint.label} className={styles.hint}>
              <span className={styles.keys}>{hint.keys}</span> {hint.label}
            </span>
          ),
      )}
    </p>
  );
}
