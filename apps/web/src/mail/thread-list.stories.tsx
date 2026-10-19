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
import type { Check } from "./thread-list-item";
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

// The largest type with the loosest leading: the rows grow with their two lines.
export const LargerAndLoose: StoryObj = {
  globals: { fontSize: "larger", lineHeight: "loose" },
  render: () => listed({}),
};

// The drawn states of a row, one under the other; hover is the
// pointer's own.
interface Drawn {
  name: string;
  row: ListRow;
  open?: boolean;
  check?: Check;
  pressing?: boolean;
  stop?: boolean;
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
    { name: "default", row: PLAIN },
    { name: "unread", row: UNREAD },
    { name: "open", row: PLAIN, open: true },
    { name: "open and unread", row: UNREAD, open: true },
    { name: "focused", row: PLAIN, stop: true },
    {
      name: "a touch resting before its press lands",
      row: UNREAD,
      check: "offered",
      pressing: true,
    },
    { name: "picked", row: PLAIN, check: "on" },
    { name: "picked and unread", row: UNREAD, check: "on" },
    { name: "picked and open", row: PLAIN, check: "on", open: true },
    { name: "not picked while a selection stands", row: UNREAD, check: "off" },
    { name: "thread, flagged, attachment", row: THREAD },
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
          stop={state.stop ?? false}
          open={state.open ?? false}
          check={state.check ?? "none"}
          pressing={state.pressing ?? false}
          actions={null}
          swipe={null}
          closing={false}
          start={index * ROW_PX}
          size={ROW_PX}
          onPlace={nothing}
          onCheck={nothing}
        />
      ))}
    </div>
  ),
};
