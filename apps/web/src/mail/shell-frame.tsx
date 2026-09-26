// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow, MailCache, ReadingPane as PanePreference } from "@huliho/core";
import type { ReactNode, RefObject } from "react";
import { useId, useLayoutEffect, useRef, useState } from "react";

import { SideSheet } from "../design-system/side-sheet";
import { DIVIDER_STEP_PX, SplitDivider, clamp } from "../design-system/split-divider";
import type { Bounds } from "../design-system/split-divider";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import type { Layout } from "../shell/breakpoints";
import { PaneBoundary } from "../shell/pane-boundary";
import { LIST_DEFAULT_ROWS, LIST_MIN_ROWS, PANE_MIN_HEIGHT_PX, useListHeight } from "./list-height";
import { ListPane } from "./list-pane";
import { LIST_WIDTH_DEFAULT_PX, PANE_MIN_WIDTH_PX, useListWidth } from "./list-width";
import { Rail } from "./rail";
import { ReadingPane, ThreadScreen } from "./reading-pane";
import type { OpenThread } from "./reading-pane";
import { Sidebar } from "./sidebar";
import type { PanePosition } from "./thread-pane";
import type { TreeState } from "./tree";
import {
  FOOT_HEIGHT_FALLBACK_PX,
  FOOT_HEIGHT_TOKEN,
  ROW_HEIGHT_FALLBACK_PX,
  ROW_HEIGHT_TOKEN,
  TOOLBAR_HEIGHT_FALLBACK_PX,
  TOOLBAR_HEIGHT_TOKEN,
  useTokenPx,
} from "./use-token-px";
import styles from "./shell-frame.module.css";

// What the sidebar, the rail and the list pane all take.
interface Shared {
  locale: Locale;
  cache: MailCache;
  accounts: readonly AccountRow[];
  account: AccountRow;
  tree: TreeState;
  currentMailboxId: string | undefined;
}

interface ShellFrameProps extends Shared {
  layout: Layout;
  readingPane: PanePreference;
  currentThreadId: string | undefined;
  onCloseThread: () => void;
  // What the list pane shows under its header.
  children: ReactNode;
}

// The list's size along the axis the seam moves on, the bounds the seam
// announces, its step and the setter behind them.
interface ListSize {
  value: number;
  bounds: Bounds;
  step: number;
  choose: (size: number | null) => void;
}

// The density's row, toolbar and foot heights, which size the list
// above the pane in rows.
interface Heights {
  row: number;
  toolbar: number;
  foot: number;
}

interface SeamProps {
  locale: Locale;
  cache: MailCache;
  position: "right" | "bottom";
  list: ListSize;
  seamRef: RefObject<HTMLDivElement | null>;
  // The id of the list pane the seam sizes.
  controls: string;
  thread: OpenThread | null;
  keyHints: boolean;
  onClose: () => void;
}

interface StageProps {
  locale: Locale;
  layout: Layout;
  cache: MailCache;
  position: PanePosition;
  list: ListSize;
  seamRef: RefObject<HTMLDivElement | null>;
  thread: OpenThread | null;
  shared: Shared;
  onOpenSidebar: () => void;
  onCloseThread: () => void;
  children: ReactNode;
}

// The panels the room is measured against: the frame, the side panel
// and the seam, which takes a band of the flow on a touchscreen.
interface Panels {
  frame: RefObject<HTMLDivElement | null>;
  side: RefObject<HTMLDivElement | null>;
  seam: RefObject<HTMLDivElement | null>;
}

// Where a conversation opens: beside the list, below it or as a screen
// of its own on a phone and wherever the pane is off.
function positionOf(layout: Layout, readingPane: PanePreference): PanePosition {
  return layout === "phone" || readingPane === "off" ? "screen" : readingPane;
}

// The thread the address names, with the mailbox it was opened from.
function openThreadOf(
  tree: TreeState,
  accountId: string,
  mailboxId: string | undefined,
  threadId: string | undefined,
): OpenThread | null {
  if (threadId === undefined) {
    return null;
  }
  const mailbox =
    tree.status === "success" ? tree.mailboxes.find((row) => row.id === mailboxId) : undefined;
  return { accountId, threadId, mailbox };
}

// What an element takes of the flow along the block axis: its box with
// its margins, which pull a seam's touch band back to its width.
function blockFlowOf(element: HTMLElement | null): number {
  if (element === null) {
    return 0;
  }
  const style = getComputedStyle(element);
  const margins =
    (parseFloat(style.marginBlockStart) || 0) + (parseFloat(style.marginBlockEnd) || 0);
  return Math.max(0, element.offsetHeight + margins);
}

// What the reading pane leaves the list: the frame less the side panel
// and the pane's least width across, the frame less the seam's band and
// the pane's least height down.
function roomFor({ frame, side, seam }: Panels, position: PanePosition): number {
  if (position === "bottom") {
    const taken = blockFlowOf(seam.current) + PANE_MIN_HEIGHT_PX;
    return Math.max(0, (frame.current?.clientHeight ?? 0) - taken);
  }
  const taken = (side.current?.offsetWidth ?? 0) + PANE_MIN_WIDTH_PX;
  return Math.max(0, (frame.current?.clientWidth ?? 0) - taken);
}

// The seam's bounds: a frame too small for the list's least and the
// pane's keeps the list at its least, so the bounds never invert.
function boundsFor(min: number, room: number): Bounds {
  return { min, max: Math.max(min, room) };
}

// The room the list may take, measured once the frame is in the DOM and
// again when the window resizes, when the side panel does (a sidebar at
// one width, a rail at another), when the seam does or when the
// position changes. A screen has no seam.
function useRoom(panels: Panels, position: PanePosition): number {
  const [room, setRoom] = useState(0);
  useLayoutEffect(() => {
    if (position === "screen") {
      return undefined;
    }
    const measure = (): void => {
      setRoom(roomFor(panels, position));
    };
    measure();
    window.addEventListener("resize", measure);
    const observer = new ResizeObserver(measure);
    for (const watched of [panels.side.current, panels.seam.current]) {
      if (watched !== null) {
        observer.observe(watched);
      }
    }
    return () => {
      window.removeEventListener("resize", measure);
      observer.disconnect();
    };
  }, [panels, position]);
  return room;
}

// The list's size at the two split positions: the device's choice or
// the design's default, clamped to the room the frame leaves. Beside
// the pane that is a width; above it a height in rows between the
// header and the foot, at the density's row, toolbar and foot heights.
function useListSize(panels: Panels, position: PanePosition): ListSize {
  const [listWidth, chooseWidth] = useListWidth();
  const [listHeight, chooseHeight] = useListHeight();
  const room = useRoom(panels, position);
  const heights: Heights = {
    row: useTokenPx(panels.frame, ROW_HEIGHT_TOKEN, ROW_HEIGHT_FALLBACK_PX),
    toolbar: useTokenPx(panels.frame, TOOLBAR_HEIGHT_TOKEN, TOOLBAR_HEIGHT_FALLBACK_PX),
    foot: useTokenPx(panels.frame, FOOT_HEIGHT_TOKEN, FOOT_HEIGHT_FALLBACK_PX),
  };
  if (position === "bottom") {
    const fixed = heights.toolbar + heights.foot;
    const bounds = boundsFor(fixed + LIST_MIN_ROWS * heights.row, room);
    const fallback = fixed + LIST_DEFAULT_ROWS * heights.row;
    return {
      value: clamp(listHeight ?? fallback, bounds),
      bounds,
      step: heights.row,
      choose: chooseHeight,
    };
  }
  const bounds = boundsFor(PANE_MIN_WIDTH_PX, room);
  return {
    value: clamp(listWidth ?? LIST_WIDTH_DEFAULT_PX, bounds),
    bounds,
    step: DIVIDER_STEP_PX,
    choose: chooseWidth,
  };
}

// The seam and the reading pane after it, beside or below the list.
function Seam(props: SeamProps) {
  const { locale, cache, position, list, seamRef, controls, thread, keyHints, onClose } = props;
  return (
    <>
      <SplitDivider
        ref={seamRef}
        label={m.mail_resize_list({}, { locale })}
        value={list.value}
        bounds={list.bounds}
        controls={controls}
        orientation={position === "bottom" ? "horizontal" : "vertical"}
        step={list.step}
        onChange={list.choose}
        onReset={() => {
          list.choose(null);
        }}
      />
      <PaneBoundary>
        <ReadingPane
          locale={locale}
          cache={cache}
          thread={thread}
          position={position}
          keyHints={keyHints}
          onClose={onClose}
        />
      </PaneBoundary>
    </>
  );
}

// The list with the reading pane beside or below it, or with the thread
// as a screen over it, where the list stays put out of reach.
function Stage(props: StageProps) {
  const { locale, layout, cache, position, list, seamRef, thread, shared, children } = props;
  const listId = useId();
  const keyHints = layout !== "phone";
  return (
    <div className={styles.stage} data-pane={position}>
      <ListPane
        {...shared}
        id={listId}
        position={position}
        size={position === "screen" ? null : list.value}
        inert={position === "screen" && thread !== null}
        onOpenSidebar={layout === "phone" ? props.onOpenSidebar : undefined}
      >
        {children}
      </ListPane>
      {position !== "screen" && (
        <Seam
          locale={locale}
          cache={cache}
          position={position}
          list={list}
          seamRef={seamRef}
          controls={listId}
          thread={thread}
          keyHints={keyHints}
          onClose={props.onCloseThread}
        />
      )}
      {position === "screen" && thread !== null && (
        <PaneBoundary>
          <ThreadScreen
            locale={locale}
            cache={cache}
            thread={thread}
            keyHints={keyHints}
            onClose={props.onCloseThread}
          />
        </PaneBoundary>
      )}
    </div>
  );
}

// The sidebar's sheet at the narrower layouts: open or closed.
interface Sheet {
  open: boolean;
  setOpen: (open: boolean) => void;
  show: () => void;
  hide: () => void;
}

function useSheet(): Sheet {
  const [open, setOpen] = useState(false);
  return {
    open,
    setOpen,
    show: () => {
      setOpen(true);
    },
    hide: () => {
      setOpen(false);
    },
  };
}

// The three layouts of the mail screen: the sidebar, the list and the
// reading pane at the desktop width, the sidebar as a rail at the tablet
// width and the list alone on a phone, with the sidebar in a sheet at
// both of those. The pane stands beside the list, below it or as a
// screen over it, as the preference and the width decide. Each pane
// keeps a render failure to itself.
export function ShellFrame(props: ShellFrameProps) {
  const { locale, layout, readingPane, cache, accounts, account, tree, children } = props;
  const { currentMailboxId, currentThreadId, onCloseThread } = props;
  const frame = useRef<HTMLDivElement>(null);
  const side = useRef<HTMLDivElement>(null);
  const seam = useRef<HTMLDivElement>(null);
  const sheet = useSheet();
  const position = positionOf(layout, readingPane);
  const list = useListSize({ frame, side, seam }, position);
  const shared = { locale, cache, accounts, account, tree, currentMailboxId };
  const sidebar = (
    <PaneBoundary>
      <Sidebar {...shared} showLetters={layout === "desktop"} onNavigate={sheet.hide} />
    </PaneBoundary>
  );
  return (
    <div ref={frame} className={styles.shell} data-layout={layout}>
      {layout !== "phone" && (
        <div ref={side} className={styles.side}>
          {layout === "desktop" ? (
            sidebar
          ) : (
            <PaneBoundary>
              <Rail {...shared} onMore={sheet.show} />
            </PaneBoundary>
          )}
        </div>
      )}
      <Stage
        locale={locale}
        layout={layout}
        cache={cache}
        position={position}
        list={list}
        seamRef={seam}
        thread={openThreadOf(tree, account.id, currentMailboxId, currentThreadId)}
        shared={shared}
        onOpenSidebar={sheet.show}
        onCloseThread={onCloseThread}
      >
        {children}
      </Stage>
      {layout !== "desktop" && (
        <SideSheet
          open={sheet.open}
          onOpenChange={sheet.setOpen}
          label={m.mail_sidebar({}, { locale })}
          closeLabel={m.sheet_close({}, { locale })}
        >
          {sidebar}
        </SideSheet>
      )}
    </div>
  );
}
