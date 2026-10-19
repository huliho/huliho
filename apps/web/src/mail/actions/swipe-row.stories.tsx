// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ListRow, MailAction } from "@huliho/core";
import type { Meta, StoryObj } from "@storybook/react-vite";

import type { Locale } from "../../paraglide/runtime.js";
import { FIXED_NOW, INBOX_PAGE } from "../fixtures";
import { startOfDay } from "../row-time";
import { ThreadListItem } from "../thread-list-item";
import styles from "../thread-list.module.css";
import { iconOf } from "./action-icon";
import type { ActionKind } from "./kinds";
import type { SwipeView, SwipeWay } from "./swipe";
import type { SwipeAction } from "./swipe-row";
import { actionLabel } from "./words";

const TODAY = startOfDay(FIXED_NOW);
// The phone the swipe is drawn at and the touch row's height there.
const PHONE_PX = 390;
const TOUCH_ROW_PX = 56;
// A quarter of the row, a third of it and half of it: the commit point.
const QUARTER_PX = PHONE_PX / 4;
const THIRD_PX = PHONE_PX / 3;
const HALF_PX = PHONE_PX / 2;

// One drawn moment of a swipe: its way, how far the row moved and the
// action behind it.
interface Moment {
  name: string;
  way: SwipeWay;
  reach: number;
  kind: ActionKind;
  action: MailAction;
  open?: boolean;
}

function viewOf(row: ListRow, index: number, { way, reach }: Moment, rtl: boolean): SwipeView {
  const offset = (way === "toEnd") !== rtl ? reach : -reach;
  const armed = reach >= HALF_PX;
  return { threadId: row.threadId, index, way, offset, armed, phase: "held" };
}

function actionOf({ kind, action }: Moment, locale: Locale): SwipeAction {
  return { kind, icon: iconOf(action), label: actionLabel(action, locale) };
}

function nothing(): void {
  // A drawn row takes no focus and settles nowhere.
}

// The moments one under the other, as a grid of touch rows at a phone's
// width; the story fills its frame, so a phone shows it whole.
function moments(drawn: readonly Moment[], locale: Locale, rtl: boolean): StoryObj {
  const row = INBOX_PAGE.rows[2];
  return {
    globals: { density: "touch" },
    parameters: { layout: "fullscreen" },
    render: () =>
      row === undefined ? (
        <></>
      ) : (
        <div dir={rtl ? "rtl" : "ltr"} style={{ inlineSize: `${String(PHONE_PX)}px` }}>
          <div
            role="grid"
            aria-label="Swipe moments"
            aria-rowcount={drawn.length}
            className={styles.sizer}
            style={{ blockSize: `${String(drawn.length * TOUCH_ROW_PX)}px` }}
          >
            {drawn.map((moment, index) => (
              <ThreadListItem
                key={moment.name}
                locale={locale}
                today={TODAY}
                row={row}
                index={index}
                stop={false}
                open={moment.open ?? false}
                check="none"
                pressing={false}
                actions={null}
                swipe={{
                  view: viewOf(row, index, moment, rtl),
                  action: actionOf(moment, locale),
                  onSettled: nothing,
                }}
                closing={false}
                start={index * TOUCH_ROW_PX}
                size={TOUCH_ROW_PX}
                onPlace={nothing}
                onCheck={nothing}
              />
            ))}
          </div>
        </div>
      ),
  };
}

const meta: Meta = {
  title: "Mail/SwipeRow",
};

export default meta;

// Archive toward the line's end and delete toward its start: the icon
// alone at a quarter of the row, the word from the commit point.
export const Ways = moments(
  [
    {
      name: "archive, a quarter",
      way: "toEnd",
      reach: QUARTER_PX,
      kind: "archive",
      action: "archive",
    },
    {
      name: "archive, the commit point",
      way: "toEnd",
      reach: HALF_PX,
      kind: "archive",
      action: "archive",
    },
    {
      name: "delete, a quarter",
      way: "toStart",
      reach: QUARTER_PX,
      kind: "delete",
      action: "delete",
    },
    {
      name: "delete, the commit point",
      way: "toStart",
      reach: HALF_PX,
      kind: "delete",
      action: "delete",
    },
    {
      name: "Trash, the commit point",
      way: "toStart",
      reach: HALF_PX,
      kind: "delete",
      action: "deleteForever",
    },
    {
      name: "the open row, a quarter",
      way: "toEnd",
      reach: QUARTER_PX,
      kind: "archive",
      action: "archive",
      open: true,
    },
  ],
  "en",
  false,
);

// The pseudo-locale reads right to left, so archive moves the row left.
export const Mirrored = moments(
  [
    { name: "archive, a third", way: "toEnd", reach: THIRD_PX, kind: "archive", action: "archive" },
    {
      name: "delete, the commit point",
      way: "toStart",
      reach: HALF_PX,
      kind: "delete",
      action: "delete",
    },
  ],
  "en-XA",
  true,
);
