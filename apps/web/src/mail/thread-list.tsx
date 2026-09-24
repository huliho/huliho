// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { firstSyncOf } from "@huliho/core";
import type { FirstSync, ListRow, MailCache, Mailbox } from "@huliho/core";
import { useRef, useState } from "react";
import type { KeyboardEvent, ReactNode, RefObject } from "react";

import { useCommand } from "../commands/use-command";
import { ErrorState } from "../design-system/error-state";
import spoken from "../design-system/spoken.module.css";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { FirstSyncBlock } from "./first-sync";
import { keyHandler, useCursor } from "./list-cursor";
import type { ListCursor } from "./list-cursor";
import { useMarker } from "./list-marker";
import type { ListMarker } from "./list-marker";
import { pagesFor, useListVirtualizer } from "./list-virtualizer";
import type { VirtualList } from "./list-virtualizer";
import { NewMailMarker } from "./new-mail-marker";
import { OfflineBanner } from "./offline-banner";
import { EdgeRow, ThreadListItem } from "./thread-list-item";
import { useRowHeight } from "./use-row-height";
import { pageOf, rowAt, useThreadPages } from "./use-thread-pages";
import type { ListPages } from "./use-thread-pages";
import styles from "./thread-list.module.css";

// Still rows after the last synced one while the first sync runs.
const SYNC_EDGE_ROWS = 3;
// Still rows that stand in for the list while its first page loads.
const LOADING_ROWS = 8;
// The key that brings new mail in; the marker shows it.
const REVEAL_KEY = ".";

interface ThreadListProps {
  locale: Locale;
  // The start of today, which the times read against.
  today: number;
  cache: MailCache;
  accountId: string;
  mailbox: Mailbox;
  online: boolean;
  // What the pane shows when the list holds no row.
  empty: ReactNode;
}

// Where the first page stands: still on its way, refused or in.
type PageState = "loading" | "failed" | "ready";

// What the first page and the mailbox say about the list as a whole.
interface Facts {
  rowCount: number;
  pending: number;
  firstSync: FirstSync | null;
  state: PageState;
  // Nothing to list and no sync on its way: the empty word shows.
  isEmpty: boolean;
}

// Rows once held stay readable, whatever a later refetch answered.
function stateOf(first: ListPages["first"]): PageState {
  if (first?.data !== undefined) {
    return "ready";
  }
  return first?.isError === true ? "failed" : "loading";
}

// A list with no row, no new mail waiting and no sync on its way.
function isEmptyList(facts: Omit<Facts, "isEmpty">): boolean {
  return (
    facts.state === "ready" &&
    facts.rowCount === 0 &&
    facts.pending === 0 &&
    facts.firstSync === null
  );
}

function factsOf(pages: ListPages, mailbox: Mailbox): Facts {
  const page = pages.first?.data;
  const facts = {
    rowCount: page?.total ?? pages.rows.get(0)?.length ?? 0,
    pending: page?.pending ?? 0,
    firstSync: firstSyncOf(mailbox),
    state: stateOf(pages.first),
  };
  return { ...facts, isEmpty: isEmptyList(facts) };
}

interface Commands {
  rowCount: number;
  pending: number;
  active: number;
  goTo: (index: number) => void;
  reveal: () => void;
}

// j and k move the cursor from anywhere on the screen; the dot key
// brings new mail in while some waits.
function useListCommands({ rowCount, pending, active, goTo, reveal }: Commands): void {
  const next = (): void => {
    goTo(active + 1);
  };
  const previous = (): void => {
    goTo(active - 1);
  };
  useCommand(rowCount > 0 ? { id: "list.next", key: "j", run: next } : null);
  useCommand(rowCount > 0 ? { id: "list.previous", key: "k", run: previous } : null);
  useCommand(pending > 0 ? { id: "list.reveal", key: REVEAL_KEY, run: reveal } : null);
}

function LoadingRows({ locale }: { locale: Locale }) {
  return (
    <div role="status" className={styles.loading} aria-label={m.loading_label({}, { locale })}>
      {Array.from({ length: LOADING_ROWS }, (_, index) => (
        <EdgeRow key={index} placement={null} index={index} />
      ))}
    </div>
  );
}

interface StateProps {
  locale: Locale;
  facts: Facts;
  retry: () => void;
  empty: ReactNode;
}

// What stands in for the rows: the error sentence with Try again while
// the first page is refused, the still rows while it loads and the
// empty word when it holds nothing.
function ListState({ locale, facts, retry, empty }: StateProps) {
  if (facts.state === "failed") {
    return (
      <ErrorState
        message={m.mail_error({}, { locale })}
        retryLabel={m.retry_action({}, { locale })}
        onRetry={retry}
      />
    );
  }
  if (facts.state === "loading") {
    return <LoadingRows locale={locale} />;
  }
  return facts.isEmpty ? empty : null;
}

interface GridProps {
  locale: Locale;
  today: number;
  scrollerRef: RefObject<HTMLDivElement | null>;
  list: VirtualList;
  rows: ReadonlyMap<number, ListRow[]>;
  rowCount: number;
  cursor: ListCursor;
  onKeyDown: (event: KeyboardEvent<HTMLElement>) => void;
  // A scroll or a click in the grid: the user acted in the list.
  onAct: () => void;
}

// The rows in view, each placed by its offset; a row whose page is on
// its way is still, and so are the rows past the synced edge.
function Grid(props: GridProps) {
  const { locale, today, scrollerRef, list, rows, rowCount, cursor } = props;
  return (
    <div
      ref={scrollerRef}
      role="grid"
      tabIndex={-1}
      aria-label={m.list_label({}, { locale })}
      aria-rowcount={rowCount}
      className={styles.scroller}
      onKeyDown={props.onKeyDown}
      onScroll={props.onAct}
      onClick={props.onAct}
    >
      <div className={styles.sizer} style={{ blockSize: `${String(list.totalSize)}px` }}>
        {list.items.map((item) =>
          item.index >= rowCount ? (
            <EdgeRow key={item.key} placement={item} index={item.index} />
          ) : (
            <ThreadListItem
              key={item.key}
              locale={locale}
              today={today}
              row={rowAt(rows, item.index)}
              index={item.index}
              stop={item.index === cursor.active}
              selection="none"
              start={item.start}
              size={item.size}
              onPlace={cursor.place}
            />
          ),
        )}
      </div>
    </div>
  );
}

interface Actions {
  cache: MailCache;
  accountId: string;
  mailboxId: string;
  pages: ListPages;
  cursor: ListCursor;
  markerRef: RefObject<HTMLElement | null>;
}

// The two actions whose own element leaves once they land: the reveal
// takes the marker away and Try again puts the grid where the button
// stood. The focus each held goes to the cursor's row.
function listActions({ cache, accountId, mailboxId, pages, cursor, markerRef }: Actions) {
  const landAgain = async (): Promise<void> => {
    if (await pages.retry()) {
      cursor.focus();
    }
  };
  return {
    reveal: (): void => {
      if (markerRef.current?.contains(document.activeElement) === true) {
        cursor.focus();
      }
      void cache.reveal(accountId, mailboxId);
    },
    retry: (): void => {
      void landAgain();
    },
  };
}

interface ControlsProps {
  locale: Locale;
  cache: MailCache;
  accountId: string;
  mailbox: Mailbox;
  pages: ListPages;
  facts: Facts;
  rowHeight: number;
  scrollerRef: RefObject<HTMLDivElement | null>;
  regionRef: RefObject<HTMLDivElement | null>;
  markerRef: RefObject<HTMLDivElement | null>;
  onSpan: (span: string) => void;
  // Told the page of the row the cursor went to, so it stays mounted.
  onPin: (page: number) => void;
}

interface ListControls {
  cursor: ListCursor;
  list: VirtualList;
  marker: ListMarker;
  onKeyDown: (event: KeyboardEvent<HTMLElement>) => void;
  reveal: () => void;
  retry: () => void;
}

// The cursor, the virtualizer, the marker and the commands over the
// pages the list holds.
function useListControls(props: ControlsProps): ListControls {
  const { locale, cache, accountId, mailbox, pages, facts, rowHeight } = props;
  const { scrollerRef, regionRef, markerRef, onSpan, onPin } = props;
  const { rowCount, pending } = facts;
  const marker = useMarker(pending, regionRef, m.list_new_mail({ count: pending }, { locale }));
  const onMove = (index: number): void => {
    marker.dismiss();
    onPin(pageOf(index));
  };
  const cursor = useCursor({ scrollerRef, rows: pages.rows, rowCount, onMove });
  const list = useListVirtualizer({
    scrollerRef,
    rowHeight,
    rowCount,
    edgeRows: facts.firstSync === null ? 0 : SYNC_EDGE_ROWS,
    active: cursor.active,
    onSpan,
  });
  const goTo = (index: number): void => {
    cursor.moveTo(index, list.scrollTo);
  };
  const actions = listActions({
    cache,
    accountId,
    mailboxId: mailbox.id,
    pages,
    cursor,
    markerRef,
  });
  useListCommands({ rowCount, pending, active: cursor.active, goTo, reveal: actions.reveal });
  return {
    cursor,
    list,
    marker,
    onKeyDown: keyHandler(cursor, rowCount, list.scrollTo),
    ...actions,
  };
}

// The threads of one mailbox as a virtualized grid over the pages the
// cache serves: one tab stop that the arrow keys, j and k move, the
// marker for new mail, the offline strip and the first-sync foot.
export function ThreadList(props: ThreadListProps) {
  const { locale, today, cache, accountId, mailbox, online, empty } = props;
  const listRef = useRef<HTMLDivElement>(null);
  const scrollerRef = useRef<HTMLDivElement>(null);
  const regionRef = useRef<HTMLDivElement>(null);
  const markerRef = useRef<HTMLDivElement>(null);
  const [span, setSpan] = useState("0-0");
  // The page of the row the cursor last went to; it stays mounted with it.
  const [pinned, setPinned] = useState(0);
  const rowHeight = useRowHeight(listRef);
  const pages = useThreadPages(cache, accountId, mailbox.id, pagesFor(span, pinned));
  const facts = factsOf(pages, mailbox);
  const controls = useListControls({
    locale,
    cache,
    accountId,
    mailbox,
    pages,
    facts,
    rowHeight,
    scrollerRef,
    regionRef,
    markerRef,
    onSpan: setSpan,
    onPin: setPinned,
  });
  return (
    <div ref={listRef} className={styles.list}>
      <OfflineBanner locale={locale} online={online} />
      <div ref={regionRef} aria-live="polite" className={spoken.spoken} />
      <ListState locale={locale} facts={facts} retry={controls.retry} empty={empty} />
      {/* The grid mounts once the first page is in: measured laid out, its rows render with it. */}
      {facts.state === "ready" && !facts.isEmpty && (
        <div className={styles.viewport}>
          {controls.marker.shown && (
            <NewMailMarker
              ref={markerRef}
              locale={locale}
              count={facts.pending}
              keyHint={REVEAL_KEY}
              onReveal={controls.reveal}
            />
          )}
          <Grid
            locale={locale}
            today={today}
            scrollerRef={scrollerRef}
            list={controls.list}
            rows={pages.rows}
            rowCount={facts.rowCount}
            cursor={controls.cursor}
            onKeyDown={controls.onKeyDown}
            onAct={controls.marker.dismiss}
          />
        </div>
      )}
      <FirstSyncBlock locale={locale} progress={facts.firstSync} />
    </div>
  );
}
