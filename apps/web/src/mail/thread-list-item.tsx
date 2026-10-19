// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow } from "@huliho/core";
import { Paperclip, Star } from "lucide-react";
import type { CSSProperties } from "react";

import { Checkbox } from "../design-system/checkbox";
import { cx } from "../design-system/cx";
import { Skeleton } from "../design-system/skeleton";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { RowActions } from "./actions/row-actions";
import type { RowActionsHandle } from "./actions/row-actions";
import { SwipeRow } from "./actions/swipe-row";
import type { RowSwipe } from "./actions/swipe-row";
import { MonoCount } from "./mono-count";
import { formatRowTime } from "./row-time";
import styles from "./thread-list-item.module.css";

// The row's check in the dot's place. An empty box is offered under a
// pointer or a press; off and on stand while a selection does.
export type Check = "none" | "offered" | "off" | "on";

// What the grid reads to tell a press on the check from a click that
// opens the row.
export const ROW_CHECK = "[data-row-check]";

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
  // Whether the row's thread is the one open in the reading pane.
  open: boolean;
  check: Check;
  // Whether a touch rests on the row before its long press lands.
  pressing: boolean;
  // The thread's actions, which a loaded row shows under a pointer and
  // while it holds the focus; null on a row that carries none.
  actions: RowActionsHandle | null;
  // The swipe that moves the row; null while none does.
  swipe: RowSwipe | null;
  // Whether the row closes up into the place of a swiped row that left.
  closing: boolean;
  // Told when the row takes focus on its own, by pointer or by Tab.
  onPlace: (index: number) => void;
  // Told of a press on the check, with whether Shift was down.
  onCheck: (shift: boolean) => void;
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

interface MarkProps {
  locale: Locale;
  check: Check;
  onCheck: (shift: boolean) => void;
}

// The unread dot, or the check where one stands. A press on the check
// leaves the focus where it stood.
function Mark({ locale, check, onCheck }: MarkProps) {
  if (check === "none") {
    return <span className={styles.dot} aria-hidden="true" />;
  }
  return (
    <span className={styles.mark} data-row-check="">
      <Checkbox
        checked={check === "on"}
        label={m.row_select({}, { locale })}
        focusable={false}
        onCheckedChange={(_checked, event) => {
          onCheck(event instanceof MouseEvent && event.shiftKey);
        }}
      />
    </span>
  );
}

interface FactsProps extends MarkProps {
  today: number;
  row: ListRow;
  index: number;
  actions: RowActionsHandle | null;
}

// The check stands where the dot sits and the actions stand in for the
// time where the row shows them. The sender and the text line are mail
// content in their own direction; the line takes the subject's, the
// preview inside it its own.
function Facts({ locale, today, row, index, check, onCheck, actions }: FactsProps) {
  return (
    <>
      <Mark locale={locale} check={check} onCheck={onCheck} />
      <span dir="auto" className={styles.sender}>
        {senderOf(row, locale)}
      </span>
      <span className={styles.count}>
        {row.count > 1 && <MonoCount value={row.count} locale={locale} tone="muted" />}
      </span>
      <span className={styles.meta}>
        {row.hasAttachment && <Paperclip className={styles.icon} aria-hidden="true" />}
        {row.flagged && <Star className={cx(styles.icon, styles.flag)} aria-hidden="true" />}
        <span className={styles.time}>{formatRowTime(row.receivedAt, today, locale)}</span>
      </span>
      {actions !== null && <RowActions locale={locale} handle={actions} row={row} index={index} />}
      <span dir="auto" className={styles.text}>
        <span className={styles.subject}>{subjectOf(row, locale)}</span>
        {row.preview !== "" && (
          <span className={styles.preview}>
            {" · "}
            <span dir="auto">{row.preview}</span>
          </span>
        )}
      </span>
    </>
  );
}

// What the row says of itself as selected. While a selection stands the
// picks are the selected rows and the open one is the current one;
// otherwise the open row is the selected one.
function selectedness(open: boolean, check: Check) {
  if (check === "on" || check === "off") {
    return { "aria-selected": check === "on", "aria-current": open || undefined };
  }
  return { "aria-selected": open || undefined };
}

// The states the row's styles read.
function marks({ row, open, pressing, swipe, closing }: ItemProps) {
  return {
    "data-unread": row?.unread === true || undefined,
    "data-open": open || undefined,
    "data-pressing": pressing || undefined,
    "data-swiping": swipe !== null || undefined,
    "data-closing": closing || undefined,
  };
}

// One thread of the list on two lines. A row whose page is on its way
// shows the still shape inside the same element, so the focus stays put
// while the page lands.
export function ThreadListItem(props: ItemProps) {
  const { locale, today, row, index, stop, open, check, actions, start, size, onPlace } = props;
  const loaded = row !== undefined;
  return (
    <div
      role="row"
      aria-rowindex={index + 1}
      aria-label={loaded ? rowName(row, locale, today) : m.loading_label({}, { locale })}
      aria-busy={loaded ? undefined : true}
      {...selectedness(open, loaded ? check : "none")}
      tabIndex={stop ? 0 : -1}
      data-index={index}
      {...marks(props)}
      className={styles.row}
      style={placed({ start, size })}
      onFocus={() => {
        onPlace(index);
      }}
    >
      <SwipeRow className={styles.cell} swipe={props.swipe}>
        {loaded ? (
          <Facts
            locale={locale}
            today={today}
            row={row}
            index={index}
            check={check}
            onCheck={props.onCheck}
            actions={actions}
          />
        ) : (
          <Bars shape={index % SKELETON_SHAPES} />
        )}
      </SwipeRow>
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
