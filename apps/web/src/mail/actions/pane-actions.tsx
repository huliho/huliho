// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ActionTarget, Mailbox } from "@huliho/core";

import { KeyCaps } from "../../commands/key-caps";
import { cx } from "../../design-system/cx";
import iconButton from "../../design-system/icon-button.module.css";
import { Tooltip } from "../../design-system/tooltip";
import type { Locale } from "../../paraglide/runtime.js";
import { iconOf } from "./action-icon";
import { useShownActions } from "./kinds";
import type { ActionKind } from "./kinds";
import { runOnOpenThread } from "./open-thread";
import styles from "./pane-actions.module.css";

// The toolbar's actions in the keyboard map's order.
const PANE_KINDS: readonly ActionKind[] = ["archive", "delete", "spam", "read", "star"];
// The moves show their label and key where the pane has the room; the
// flips name a state that changes under them, so they stay icons.
const LABELED: ReadonlySet<ActionKind> = new Set(["archive", "delete", "spam"]);

interface PaneActionsProps {
  locale: Locale;
  // The mailbox the thread was opened from, whose words the actions take.
  mailbox: Mailbox;
  thread: Pick<ActionTarget, "unread" | "flagged">;
  // Whether a rule parts the actions from the way back before them.
  parted?: boolean;
}

// The open thread's actions at the head of the reading pane, each with a
// tooltip that names its label and its keys. A press acts on the open
// thread whatever the focus points at.
export function PaneActions({ locale, mailbox, thread, parted = false }: PaneActionsProps) {
  const shown = useShownActions(PANE_KINDS, mailbox, thread, locale);
  if (shown.length === 0) {
    return null;
  }
  return (
    <div className={styles.actions} data-parted={parted || undefined}>
      {shown.map((entry) => {
        const Icon = iconOf(entry.action);
        return (
          <Tooltip key={entry.id} label={entry.label} keys={entry.keys}>
            <button
              type="button"
              className={cx(iconButton.button, styles.button)}
              data-labeled={LABELED.has(entry.kind) || undefined}
              onClick={() => {
                runOnOpenThread(entry.id);
              }}
            >
              <Icon
                className={cx(iconButton.icon, styles.icon)}
                data-filled={entry.action === "unstar" || undefined}
                aria-hidden="true"
              />
              <span className={styles.label}>{entry.label}</span>
              {LABELED.has(entry.kind) && (
                <span className={styles.keys}>
                  <KeyCaps keys={entry.keys} spoken={false} />
                </span>
              )}
            </button>
          </Tooltip>
        );
      })}
    </div>
  );
}
