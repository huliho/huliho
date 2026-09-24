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
import { OfflineBanner } from "./offline-banner";
import { REVEAL_KEY, Viewport } from "./thread-grid";
import { EdgeRow } from "./thread-list-item";
import { pageOf, rowAt, useThreadPages } from "./use-thread-pages";
import { useRowHeight } from "./use-token-px";
import type { ListPages } from "./use-thread-pages";
import styles from "./thread-list.module.css";

// Still rows after the last synced one while the first sync runs.
const SYNC_EDGE_ROWS = 3;
// Still rows that stand in for the list while its first page loads.
const LOADING_ROWS = 8;
// The key that opens the cursor's row from anywhere, as Enter does in the grid.
const OPEN_KEY = "o";

interface ThreadListProps {
  locale: Locale;
  // The start of today, which the times read against.
  today: number;
  cache: MailCache;
  accountId: string;
  mailbox: Mailbox;
  online: boolean;
  // The thread open beside the list, whose row is drawn as selected.
  openThreadId: string | null;
  // What the pane shows when the list holds no row.
  empty: ReactNode;
  onOpen: (row: ListRow) => void;
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
  open: () => void;
  reveal: () => void;
}

// j and k move the cursor from anywhere on the screen and o opens its
// row; the dot key brings new mail in while some waits.
function useListCommands({ rowCount, pending, active, goTo, open, reveal }: Commands): void {
  const next = (): void => {
    goTo(active + 1);
  };
  const previous = (): void => {
    goTo(active - 1);
  };
  useCommand(rowCount > 0 ? { id: "list.next", key: "j", run: next } : null);
  useCommand(rowCount > 0 ? { id: "list.previous", key: "k", run: previous } : null);
  useCommand(rowCount > 0 ? { id: "list.open", key: OPEN_KEY, run: open } : null);
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
  openThreadId: string | null;
  scrollerRef: RefObject<HTMLDivElement | null>;
  regionRef: RefObject<HTMLDivElement | null>;
  markerRef: RefObject<HTMLDivElement | null>;
  onSpan: (span: string) => void;
  // Told the page of the row the cursor went to, so it stays mounted.
  onPin: (page: number) => void;
  onOpen: (row: ListRow) => void;
}

interface ListControls {
  cursor: ListCursor;
  list: VirtualList;
  marker: ListMarker;
  onKeyDown: (event: KeyboardEvent<HTMLElement>) => void;
  // Opens the row at an index; a still row opens nothing.
  openAt: (index: number) => void;
  reveal: () => void;
  retry: () => void;
}

// The cursor, the virtualizer, the marker and the commands over the
// pages the list holds.
function useListControls(props: ControlsProps): ListControls {
  const { locale, cache, accountId, mailbox, pages, facts, rowHeight, openThreadId } = props;
  const { scrollerRef, regionRef, markerRef, onSpan, onPin, onOpen } = props;
  const { rowCount, pending } = facts;
  const marker = useMarker(pending, regionRef, m.list_new_mail({ count: pending }, { locale }));
  const onMove = (index: number): void => {
    marker.dismiss();
    onPin(pageOf(index));
  };
  const cursor = useCursor({ scrollerRef, rows: pages.rows, rowCount, openThreadId, onMove });
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
  const openAt = (index: number): void => {
    const row = rowAt(pages.rows, index);
    if (row !== undefined) {
      onOpen(row);
    }
  };
  const open = (): void => {
    openAt(cursor.active);
  };
  const actions = listActions({
    cache,
    accountId,
    mailboxId: mailbox.id,
    pages,
    cursor,
    markerRef,
  });
  useListCommands({ rowCount, pending, active: cursor.active, goTo, open, reveal: actions.reveal });
  return {
    cursor,
    list,
    marker,
    onKeyDown: keyHandler(cursor, rowCount, list.scrollTo, open),
    openAt,
    ...actions,
  };
}

// The threads of one mailbox as a virtualized grid over the pages the
// cache serves: one tab stop that the arrow keys, j and k move and
// Enter, o or a click opens, the marker for new mail, the offline strip
// and the first-sync foot.
export function ThreadList(props: ThreadListProps) {
  const { locale, today, cache, accountId, mailbox, online, openThreadId, empty, onOpen } = props;
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
    openThreadId,
    scrollerRef,
    regionRef,
    markerRef,
    onSpan: setSpan,
    onPin: setPinned,
    onOpen,
  });
  return (
    <div ref={listRef} className={styles.list}>
      <OfflineBanner locale={locale} online={online} />
      <div ref={regionRef} aria-live="polite" className={spoken.spoken} />
      <ListState locale={locale} facts={facts} retry={controls.retry} empty={empty} />
      {/* The grid mounts once the first page is in: measured laid out, its rows render with it. */}
      {facts.state === "ready" && !facts.isEmpty && (
        <Viewport
          locale={locale}
          today={today}
          scrollerRef={scrollerRef}
          markerRef={markerRef}
          marker={controls.marker}
          list={controls.list}
          rows={pages.rows}
          rowCount={facts.rowCount}
          pending={facts.pending}
          cursor={controls.cursor}
          openThreadId={openThreadId}
          onKeyDown={controls.onKeyDown}
          onOpen={controls.openAt}
          onReveal={controls.reveal}
        />
      )}
      <FirstSyncBlock locale={locale} progress={facts.firstSync} />
    </div>
  );
}
