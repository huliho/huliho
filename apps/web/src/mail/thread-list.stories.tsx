// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { JmapError } from "@huliho/core";
import type { ListRow, Mailbox } from "@huliho/core";
import type { Meta, StoryObj } from "@storybook/react-vite";
import type { JSX } from "react";

import { FIXED_NOW, INBOX_ID, INBOX_PAGE, MAILBOXES, pageOf } from "./fixtures";
import { startOfDay } from "./row-time";
import { routed } from "./story-router";
import { FixtureList } from "./story-list";
import type { FixtureListProps } from "./story-list";
import { ThreadListItem } from "./thread-list-item";
import type { Selection } from "./thread-list-item";
import styles from "./thread-list.module.css";

const TODAY = startOfDay(FIXED_NOW);
const INBOX = MAILBOXES.find((mailbox) => mailbox.id === INBOX_ID) ?? MAILBOXES[0];
// A bridge inbox a third of the way through its first sync.
const SYNCING: Mailbox | undefined =
  INBOX === undefined
    ? undefined
    : { ...INBOX, totalEmails: 18_532, unreadEmails: 212, syncedEmails: 1240 };
const NEW_MAIL = 2;
// The comfortable row height, which the row strip draws at.
const ROW_PX = 52;

// The list pane's box, so the list has a height to scroll in.
function Pane({ children }: { children: JSX.Element }): JSX.Element {
  return (
    <div style={{ display: "flex", flexDirection: "column", blockSize: "70vh" }}>{children}</div>
  );
}

function listed(props: FixtureListProps): JSX.Element {
  return routed(() => (
    <Pane>
      <FixtureList {...props} />
    </Pane>
  ));
}

const meta: Meta = {
  title: "Mail/ThreadList",
};

export default meta;

export const Default: StoryObj = {
  render: () => listed({}),
};

export const Loading: StoryObj = {
  render: () => listed({ page: "never" }),
};

export const Failed: StoryObj = {
  render: () => listed({ page: new JmapError("unavailable") }),
};

export const Offline: StoryObj = {
  render: () => listed({ online: false }),
};

export const NewMail: StoryObj = {
  render: () => listed({ page: pageOf(undefined, NEW_MAIL) }),
};

export const FirstSync: StoryObj = {
  render: () => listed({ mailbox: SYNCING }),
};

// The seven drawn states of a row, one under the other; hover is the
// pointer's own.
interface Drawn {
  name: string;
  row: ListRow;
  selection: Selection;
  stop: boolean;
}

const ROWS = INBOX_PAGE.rows;
const PLAIN = ROWS[6] ?? ROWS[0];
const UNREAD = ROWS[0];
const THREAD = ROWS[2];

function drawn(): Drawn[] {
  if (PLAIN === undefined || UNREAD === undefined || THREAD === undefined) {
    return [];
  }
  return [
    { name: "default", row: PLAIN, selection: "none", stop: false },
    { name: "unread", row: UNREAD, selection: "none", stop: false },
    { name: "selected", row: PLAIN, selection: "selected", stop: false },
    { name: "selected and unread", row: UNREAD, selection: "selected", stop: false },
    { name: "focused", row: PLAIN, selection: "none", stop: true },
    { name: "multi-selected", row: PLAIN, selection: "multi", stop: false },
    { name: "thread, flagged, attachment", row: THREAD, selection: "none", stop: false },
  ];
}

function nothing(): void {
  // A drawn row takes no focus of its own.
}

export const RowStates: StoryObj = {
  render: () => (
    <div
      role="grid"
      aria-label="Row states"
      aria-rowcount={drawn().length}
      className={styles.sizer}
      style={{ blockSize: `${String(drawn().length * ROW_PX)}px` }}
    >
      {drawn().map((state, index) => (
        <ThreadListItem
          key={state.name}
          locale="en"
          today={TODAY}
          row={state.row}
          index={index}
          stop={state.stop}
          selection={state.selection}
          start={index * ROW_PX}
          size={ROW_PX}
          onPlace={nothing}
        />
      ))}
    </div>
  ),
};
