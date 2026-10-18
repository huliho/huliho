// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow, Mailbox } from "@huliho/core";

import { KeyCaps } from "../../commands/key-caps";
import type { Locale } from "../../paraglide/runtime.js";
import { iconOf } from "./action-icon";
import { useShownActions } from "./kinds";
import type { ActionKind } from "./kinds";
import styles from "./row-actions.module.css";

// What the grid reads to tell a press on an action from a click that
// opens the row.
export const ROW_ACTIONS = "[data-row-actions]";

// The four the row offers, in the keyboard map's order: spam and move
// stay with the keys, the toolbar and the palette.
const ROW_KINDS: readonly ActionKind[] = ["archive", "delete", "read", "star"];

// What the list lends a row: the mailbox the words follow and the call
// that runs an action on the row's thread.
export interface RowActionsHandle {
  mailbox: Mailbox;
  run: (kind: ActionKind, row: ListRow, index: number) => void;
}

interface RowActionsProps {
  locale: Locale;
  handle: RowActionsHandle;
  row: ListRow;
  index: number;
}

// The thread's actions at the end of the row's first line, kept out of
// the tab order since the keys and the palette are the keyboard route.
// A press leaves the focus where it stands.
export function RowActions({ locale, handle, row, index }: RowActionsProps) {
  const shown = useShownActions(ROW_KINDS, handle.mailbox, row, locale);
  if (shown.length === 0) {
    return null;
  }
  return (
    <span className={styles.strip} data-row-actions="">
      {shown.map((entry) => {
        const Icon = iconOf(entry.action);
        return (
          <button
            key={entry.id}
            type="button"
            tabIndex={-1}
            aria-label={entry.label}
            className={styles.button}
            onMouseDown={(event) => {
              event.preventDefault();
            }}
            onClick={() => {
              handle.run(entry.kind, row, index);
            }}
          >
            <Icon
              className={styles.icon}
              data-filled={entry.action === "unstar" || undefined}
              aria-hidden="true"
            />
            <span className={styles.keys}>
              <KeyCaps keys={entry.keys} spoken={false} />
            </span>
          </button>
        );
      })}
    </span>
  );
}
