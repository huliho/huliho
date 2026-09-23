// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow } from "@huliho/core";
import type { ReactNode, RefObject } from "react";
import { useId, useLayoutEffect, useRef, useState } from "react";

import { SideSheet } from "../design-system/side-sheet";
import { SplitDivider, clamp } from "../design-system/split-divider";
import type { Bounds } from "../design-system/split-divider";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import type { Layout } from "../shell/breakpoints";
import { ListPane } from "./list-pane";
import { LIST_WIDTH_DEFAULT_PX, PANE_MIN_WIDTH_PX, useListWidth } from "./list-width";
import { Rail } from "./rail";
import { ReadingPane } from "./reading-pane";
import { Sidebar } from "./sidebar";
import type { TreeState } from "./tree";
import styles from "./shell-frame.module.css";

interface ShellFrameProps {
  locale: Locale;
  layout: Layout;
  accounts: readonly AccountRow[];
  account: AccountRow;
  tree: TreeState;
  currentMailboxId: string | undefined;
  // What the list pane shows under its header.
  children: ReactNode;
}

// The list's width, the bounds the seam announces and the setter behind them.
interface ListSize {
  width: number;
  bounds: Bounds;
  choose: (width: number | null) => void;
}

interface SeamProps {
  locale: Locale;
  list: ListSize;
  // The id of the list pane the seam sizes.
  controls: string;
}

// The list may grow until the reading pane is down to its minimum.
function roomFor(frame: HTMLElement | null, side: HTMLElement | null): number {
  const room = (frame?.clientWidth ?? 0) - (side?.offsetWidth ?? 0) - PANE_MIN_WIDTH_PX;
  return Math.max(PANE_MIN_WIDTH_PX, room);
}

// The room the list may take, measured once the frame is in the DOM and
// again when the window or the layout changes. The phone has no seam.
function useRoom(
  frame: RefObject<HTMLDivElement | null>,
  side: RefObject<HTMLDivElement | null>,
  layout: Layout,
): number {
  const [room, setRoom] = useState(PANE_MIN_WIDTH_PX);
  useLayoutEffect(() => {
    if (layout === "phone") {
      return undefined;
    }
    const measure = (): void => {
      setRoom(roomFor(frame.current, side.current));
    };
    measure();
    window.addEventListener("resize", measure);
    return () => {
      window.removeEventListener("resize", measure);
    };
  }, [frame, side, layout]);
  return room;
}

// The list's width at the two wider layouts: the device's choice or the
// design's default, clamped to the room the frame leaves.
function useListSize(
  frame: RefObject<HTMLDivElement | null>,
  side: RefObject<HTMLDivElement | null>,
  layout: Layout,
): ListSize {
  const [listWidth, choose] = useListWidth();
  const room = useRoom(frame, side, layout);
  const bounds = { min: PANE_MIN_WIDTH_PX, max: room };
  return { width: clamp(listWidth ?? LIST_WIDTH_DEFAULT_PX, bounds), bounds, choose };
}

// The seam and the reading pane after it, at the two wider layouts.
function Seam({ locale, list, controls }: SeamProps) {
  return (
    <>
      <SplitDivider
        label={m.mail_resize_list({}, { locale })}
        value={list.width}
        bounds={list.bounds}
        controls={controls}
        onChange={list.choose}
        onReset={() => {
          list.choose(null);
        }}
      />
      <ReadingPane locale={locale} />
    </>
  );
}

// The three layouts of the mail screen: the sidebar, the list and the
// reading pane side by side at the desktop width, the sidebar as a rail
// at the tablet width and the list alone on a phone, with the sidebar in
// a sheet at both of those.
export function ShellFrame(props: ShellFrameProps) {
  const { locale, layout, accounts, account, tree, currentMailboxId, children } = props;
  const frame = useRef<HTMLDivElement>(null);
  const side = useRef<HTMLDivElement>(null);
  const listId = useId();
  const [sheetOpen, setSheetOpen] = useState(false);
  const list = useListSize(frame, side, layout);
  const openSheet = (): void => {
    setSheetOpen(true);
  };
  const shared = { locale, accounts, account, tree, currentMailboxId };
  const sidebar = (
    <Sidebar
      {...shared}
      showLetters={layout === "desktop"}
      onNavigate={() => {
        setSheetOpen(false);
      }}
    />
  );
  return (
    <div ref={frame} className={styles.shell} data-layout={layout}>
      {layout !== "phone" && (
        <div ref={side} className={styles.side}>
          {layout === "desktop" ? sidebar : <Rail {...shared} onMore={openSheet} />}
        </div>
      )}
      <ListPane
        {...shared}
        id={listId}
        layout={layout}
        width={layout === "phone" ? null : list.width}
        onOpenSidebar={layout === "phone" ? openSheet : undefined}
      >
        {children}
      </ListPane>
      {layout !== "phone" && <Seam locale={locale} list={list} controls={listId} />}
      {layout !== "desktop" && (
        <SideSheet
          open={sheetOpen}
          onOpenChange={setSheetOpen}
          label={m.mail_sidebar({}, { locale })}
          closeLabel={m.sheet_close({}, { locale })}
        >
          {sidebar}
        </SideSheet>
      )}
    </div>
  );
}
