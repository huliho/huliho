// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";

import { cx } from "../design-system/cx";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { countOf } from "./tree";
import styles from "./unread-count.module.css";

interface UnreadCountProps {
  value: number;
  locale: Locale;
  // Accent on the mailbox that is open, quiet everywhere else.
  tone: "accent" | "muted";
}

// A count in mono, the way the tree, the rail and the pane header show it.
export function UnreadCount({ value, locale, tone }: UnreadCountProps) {
  return (
    <span className={cx(styles.count, tone === "accent" ? styles.accent : styles.muted)}>
      {new Intl.NumberFormat(locale).format(value)}
    </span>
  );
}

// The mailbox as a screen reader hears it: its name and, where the count
// shows, what the count counts.
export function countLabel(mailbox: Mailbox, locale: Locale): string {
  const count = countOf(mailbox);
  if (count === 0) {
    return mailbox.name;
  }
  return mailbox.role === "drafts"
    ? m.tree_row_drafts({ name: mailbox.name, count }, { locale })
    : m.tree_row_unread({ name: mailbox.name, unread: count }, { locale });
}
