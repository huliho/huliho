// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";

import { cx } from "../design-system/cx";
import spoken from "../design-system/spoken.module.css";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { countOf } from "./tree";
import styles from "./mono-count.module.css";

interface MonoCountProps {
  value: number;
  locale: Locale;
  // Accent on the mailbox that is open, quiet everywhere else.
  tone: "accent" | "muted";
  // What the number counts, for a reader who cannot see where it stands.
  label?: string | undefined;
}

// A count in mono, the way the tree, the rail, the pane header and a
// list row show it.
export function MonoCount({ value, locale, tone, label }: MonoCountProps) {
  const shown = new Intl.NumberFormat(locale).format(value);
  return (
    <span className={cx(styles.count, tone === "accent" ? styles.accent : styles.muted)}>
      {label === undefined ? shown : <span aria-hidden="true">{shown}</span>}
      {label !== undefined && <span className={spoken.spoken}>{label}</span>}
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

// The count alone in words, for the header beside the mailbox's name.
export function countWords(mailbox: Mailbox, locale: Locale): string {
  const count = countOf(mailbox);
  return mailbox.role === "drafts"
    ? m.header_drafts({ count }, { locale })
    : m.header_unread({ unread: count }, { locale });
}
