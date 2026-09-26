// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Dialog } from "../design-system/dialog";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { groupedCommands } from "./grouping";
import type { CommandGrouping } from "./grouping";
import { KeyCaps } from "./key-caps";
import type { Command } from "./registry";
import styles from "./shortcut-overlay.module.css";

interface ShortcutOverlayProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onClosed?: (() => void) | undefined;
  locale: Locale;
  commands: readonly Command[];
}

function Column({ grouping, locale }: { grouping: CommandGrouping; locale: Locale }) {
  return (
    <section className={styles.column}>
      <h3 className={styles.heading}>{grouping.label}</h3>
      <dl className={styles.rows}>
        {grouping.commands.map((command) => (
          <div key={command.id} className={styles.row}>
            <dt className={styles.term}>{command.label}</dt>
            <dd className={styles.keys}>
              <KeyCaps keys={command.keys} />
            </dd>
          </div>
        ))}
      </dl>
      {grouping.group === "go" && (
        <p className={styles.note}>{m.shortcuts_jump_note({}, { locale })}</p>
      )}
    </section>
  );
}

// Every registered command with a key, by group, as the palette lists
// them; a command the palette alone reaches has no key to show here.
export function ShortcutOverlay(props: ShortcutOverlayProps) {
  const { open, onOpenChange, onClosed, locale, commands } = props;
  const groupings = groupedCommands(
    commands.filter((command) => command.keys.length > 0),
    locale,
  );
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      onClosed={onClosed}
      title={m.shortcuts_title({}, { locale })}
      description={m.shortcuts_note({}, { locale })}
      size="wide"
    >
      <div className={styles.columns}>
        {groupings.map((grouping) => (
          <Column key={grouping.group} grouping={grouping} locale={locale} />
        ))}
      </div>
    </Dialog>
  );
}
