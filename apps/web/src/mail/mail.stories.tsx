// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ReadingPane } from "@huliho/core";
import type { Meta, StoryObj } from "@storybook/react-vite";
import type { JSX, ReactNode } from "react";

import { useLayout } from "../shell/breakpoints";
import {
  ACCOUNTS,
  FASTMAIL,
  INBOX_ID,
  INBOX_PAGE,
  MAILBOXES,
  MAILBOXES_EMPTY_INBOX,
  THREAD,
  THREAD_ID,
  fixtureCache,
} from "./fixtures";
import { EmptyMailbox } from "./mailbox-pane";
import { ShellFrame } from "./shell-frame";
import { Sidebar } from "./sidebar";
import { FixtureList } from "./story-list";
import { routed } from "./story-router";
import type { TreeState } from "./tree";

function nothing(): void {
  // Stories render states; nothing runs.
}

const LOADED: TreeState = { status: "success", mailboxes: MAILBOXES };
const LOADING: TreeState = { status: "pending" };
const FAILED: TreeState = { status: "error", retry: nothing };
const EMPTY_INBOX: TreeState = { status: "success", mailboxes: MAILBOXES_EMPTY_INBOX };
const INBOX = MAILBOXES_EMPTY_INBOX.find((mailbox) => mailbox.role === "inbox");
const THREAD_PATH = `/mail/acc-1/mb-inbox/${THREAD_ID}`;
const CACHE = fixtureCache({ [`${INBOX_ID}/0`]: INBOX_PAGE }, { [THREAD_ID]: THREAD });

interface ShellProps {
  tree: TreeState;
  currentMailboxId?: string | undefined;
  // The thread open in the frame, drawn where the preference puts it.
  currentThreadId?: string | undefined;
  readingPane?: ReadingPane;
  children?: ReactNode;
}

function Shell(props: ShellProps): JSX.Element {
  const { tree, currentMailboxId, currentThreadId, readingPane = "right", children } = props;
  const layout = useLayout();
  return (
    <ShellFrame
      locale="en"
      layout={layout}
      readingPane={readingPane}
      cache={CACHE}
      accounts={ACCOUNTS}
      account={FASTMAIL}
      tree={tree}
      currentMailboxId={currentMailboxId}
      currentThreadId={currentThreadId}
      onCloseThread={nothing}
    >
      {children}
    </ShellFrame>
  );
}

// The shell with the inbox open at its thread, the pane where `readingPane` puts it.
function opened(readingPane: ReadingPane): JSX.Element {
  return routed(
    () => (
      <Shell
        tree={LOADED}
        currentMailboxId={INBOX_ID}
        currentThreadId={THREAD_ID}
        readingPane={readingPane}
      >
        <FixtureList openThreadId={THREAD_ID} />
      </Shell>
    ),
    THREAD_PATH,
  );
}

const meta: Meta = {
  title: "Mail/Shell",
};

export default meta;

export const Default: StoryObj = {
  render: () => routed(() => <Shell tree={LOADED} currentMailboxId="mb-inbox" />),
};

export const Loading: StoryObj = {
  render: () => routed(() => <Shell tree={LOADING} />),
};

export const Failed: StoryObj = {
  render: () => routed(() => <Shell tree={FAILED} />),
};

export const EmptyInbox: StoryObj = {
  render: () =>
    routed(() => (
      <Shell tree={EMPTY_INBOX} currentMailboxId={INBOX?.id}>
        {INBOX !== undefined && (
          <EmptyMailbox
            locale="en"
            accountId={FASTMAIL.id}
            mailbox={INBOX}
            mailboxes={MAILBOXES_EMPTY_INBOX}
          />
        )}
      </Shell>
    )),
};

export const Folder: StoryObj = {
  render: () =>
    routed(() => <Shell tree={LOADED} currentMailboxId="mb-offertes" />, "/mail/acc-1/mb-offertes"),
};

export const ThreadOpen: StoryObj = {
  render: () => opened("right"),
};

export const PaneBottom: StoryObj = {
  render: () => opened("bottom"),
};

export const PaneOff: StoryObj = {
  render: () => opened("off"),
};

export const SidebarAlone: StoryObj = {
  name: "Sidebar",
  render: () =>
    routed(
      () => (
        <Sidebar
          locale="en"
          accounts={ACCOUNTS}
          account={FASTMAIL}
          tree={LOADED}
          currentMailboxId="mb-facturen"
          showLetters
        />
      ),
      "/mail/acc-1/mb-facturen",
    ),
};
