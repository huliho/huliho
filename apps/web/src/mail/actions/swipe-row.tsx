// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";
import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

import { cx } from "../../design-system/cx";
import type { Locale } from "../../paraglide/runtime.js";
import { iconOf } from "./action-icon";
import { useShownActions } from "./kinds";
import type { ActionKind } from "./kinds";
import type { SwipeView, SwipeWay } from "./swipe";
import styles from "./swipe-row.module.css";

// Archive and delete read nothing of the thread's own state.
const ANY_THREAD = { unread: false, flagged: false };

// What a way does and shows behind the row: its action's icon and word.
export interface SwipeAction {
  kind: ActionKind;
  icon: LucideIcon;
  label: string;
}

// The swipe on one row: where it stands, what its way reveals and the
// call that tells the slide home has ended.
export interface RowSwipe {
  view: SwipeView;
  action: SwipeAction | undefined;
  onSettled: () => void;
}

// Archive from start to end and delete the other way, each while the
// registry holds its command; no way while a selection stands.
export function useSwipeActions(
  mailbox: Mailbox,
  locale: Locale,
  picking: boolean,
): ReadonlyMap<SwipeWay, SwipeAction> {
  const shown = useShownActions(["archive", "delete"], mailbox, ANY_THREAD, locale);
  if (picking) {
    return new Map();
  }
  return new Map(
    shown.map(({ kind, action, label }) => [
      kind === "archive" ? "toEnd" : "toStart",
      { kind, icon: iconOf(action), label },
    ]),
  );
}

interface SwipeRowProps {
  // The cell's own class, which the swipe moves.
  className: string | undefined;
  swipe: RowSwipe | null;
  children: ReactNode;
}

// A row's cell, which a swipe moves off its place with the way's action
// behind it: the icon alone, the word from the commit point on.
export function SwipeRow({ className, swipe, children }: SwipeRowProps) {
  const view = swipe?.view;
  const action = swipe?.action;
  return (
    <>
      <div
        role="gridcell"
        className={cx(className, view === undefined ? undefined : styles.moving)}
        data-phase={view?.phase}
        style={
          view === undefined ? undefined : { transform: `translateX(${String(view.offset)}px)` }
        }
        onTransitionEnd={(event) => {
          if (event.target === event.currentTarget) {
            swipe?.onSettled();
          }
        }}
      >
        {children}
      </div>
      {view !== undefined && action !== undefined && (
        <div aria-hidden="true" className={styles.ground} data-way={view.way}>
          <action.icon className={styles.icon} />
          {view.armed && <span className={styles.word}>{action.label}</span>}
        </div>
      )}
    </>
  );
}
