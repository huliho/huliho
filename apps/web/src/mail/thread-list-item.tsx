// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow } from "@huliho/core";
import { Paperclip, SquareCheck, Star } from "lucide-react";
import type { CSSProperties } from "react";

import { cx } from "../design-system/cx";
import { Skeleton } from "../design-system/skeleton";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { MonoCount } from "./mono-count";
import { formatRowTime } from "./row-time";
import styles from "./thread-list-item.module.css";

// How a row stands apart: the one open in the reading pane, or one of
// several picked for a bulk action.
export type Selection = "none" | "selected" | "multi";

// The still rows come in three shapes, so a run of them reads as rows.
const SKELETON_SHAPES = 3;

// Where a row sits in the list's height and how tall it is, in CSS pixels.
interface Placement {
  start: number;
  size: number;
}

interface ItemProps extends Placement {
  locale: Locale;
  today: number;
  // The thread, or nothing while its page is on its way.
  row: ListRow | undefined;
  index: number;
  // Whether this is the one row Tab lands on.
  stop: boolean;
  selection: Selection;
  // Told when the row takes focus on its own, by pointer or by Tab.
  onPlace: (index: number) => void;
}

function placed({ start, size }: Placement): CSSProperties {
  return { blockSize: `${String(size)}px`, transform: `translateY(${String(start)}px)` };
}

function subjectOf(row: ListRow, locale: Locale): string {
  return row.subject === null || row.subject.trim() === ""
    ? m.list_no_subject({}, { locale })
    : row.subject;
}

function senderOf(row: ListRow, locale: Locale): string {
  return row.sender ?? m.list_no_sender({}, { locale });
}

// The row as a screen reader hears it in one go: sender, subject, time,
// the thread's size above one and the unread word.
function rowName(row: ListRow, locale: Locale, today: number): string {
  const facts = {
    sender: senderOf(row, locale),
    subject: subjectOf(row, locale),
    time: formatRowTime(row.receivedAt, today, locale),
  };
  if (row.count > 1) {
    const counted = { ...facts, count: row.count };
    return row.unread
      ? m.list_row_thread_unread(counted, { locale })
      : m.list_row_thread_read(counted, { locale });
  }
  return row.unread ? m.list_row_unread(facts, { locale }) : m.list_row_read(facts, { locale });
}

function Bars({ shape }: { shape: number }) {
  return (
    <>
      <span className={styles.dot} aria-hidden="true" />
      <span className={cx(styles.sender, styles.barLine)} data-shape={shape}>
        <Skeleton className={styles.bar} />
      </span>
      <span className={cx(styles.text, styles.barLine)} data-shape={shape}>
        <Skeleton className={styles.bar} />
      </span>
    </>
  );
}

interface FactsProps {
  locale: Locale;
  today: number;
  row: ListRow;
  selection: Selection;
}

// Sender and time on the first line; subject and preview on the second.
// A row picked among several carries the check where the dot sits.
function Facts({ locale, today, row, selection }: FactsProps) {
  return (
    <>
      {selection === "multi" ? (
        <SquareCheck className={styles.check} aria-hidden="true" />
      ) : (
        <span className={styles.dot} aria-hidden="true" />
      )}
      <span className={styles.sender}>{senderOf(row, locale)}</span>
      <span className={styles.count}>
        {row.count > 1 && <MonoCount value={row.count} locale={locale} tone="muted" />}
      </span>
      <span className={styles.meta}>
        {row.hasAttachment && <Paperclip className={styles.icon} aria-hidden="true" />}
        {row.flagged && <Star className={cx(styles.icon, styles.flag)} aria-hidden="true" />}
        <span className={styles.time}>{formatRowTime(row.receivedAt, today, locale)}</span>
      </span>
      <span className={styles.text}>
        <span className={styles.subject}>{subjectOf(row, locale)}</span>
        {row.preview !== "" && <span className={styles.preview}> · {row.preview}</span>}
      </span>
    </>
  );
}

// One thread of the list on two lines. A row whose page is on its way
// shows the still shape inside the same element, so the focus stays put
// while the page lands.
export function ThreadListItem(props: ItemProps) {
  const { locale, today, row, index, stop, selection, start, size, onPlace } = props;
  const loaded = row !== undefined;
  return (
    <div
      role="row"
      aria-rowindex={index + 1}
      aria-label={loaded ? rowName(row, locale, today) : m.loading_label({}, { locale })}
      aria-busy={loaded ? undefined : true}
      aria-selected={selection === "none" ? undefined : true}
      tabIndex={stop ? 0 : -1}
      data-index={index}
      data-unread={(loaded && row.unread) || undefined}
      data-selection={selection === "none" ? undefined : selection}
      className={styles.row}
      style={placed({ start, size })}
      onFocus={() => {
        onPlace(index);
      }}
    >
      <div role="gridcell" className={styles.cell}>
        {loaded ? (
          <Facts locale={locale} today={today} row={row} selection={selection} />
        ) : (
          <Bars shape={index % SKELETON_SHAPES} />
        )}
      </div>
    </div>
  );
}

interface EdgeRowProps {
  // Where the row sits; null for a row in the page's own flow.
  placement: Placement | null;
  // The row's place in its run, which picks one of the three shapes.
  index: number;
}

// A still row that is nobody's: the unsynced edge after the last synced
// row and the shape of the list before its first page is in.
export function EdgeRow({ placement, index }: EdgeRowProps) {
  return (
    <div
      aria-hidden="true"
      className={styles.row}
      style={placement === null ? undefined : placed(placement)}
    >
      <div className={styles.cell}>
        <Bars shape={index % SKELETON_SHAPES} />
      </div>
    </div>
  );
}
