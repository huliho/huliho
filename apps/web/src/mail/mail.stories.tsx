// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";
import type { JSX, ReactNode } from "react";

import { useLayout } from "../shell/breakpoints";
import { ACCOUNTS, FASTMAIL, MAILBOXES, MAILBOXES_EMPTY_INBOX } from "./fixtures";
import { EmptyMailbox } from "./mailbox-pane";
import { ShellFrame } from "./shell-frame";
import { Sidebar } from "./sidebar";
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

interface ShellProps {
  tree: TreeState;
  currentMailboxId?: string | undefined;
  children?: ReactNode;
}

function Shell({ tree, currentMailboxId, children }: ShellProps): JSX.Element {
  const layout = useLayout();
  return (
    <ShellFrame
      locale="en"
      layout={layout}
      accounts={ACCOUNTS}
      account={FASTMAIL}
      tree={tree}
      currentMailboxId={currentMailboxId}
    >
      {children}
    </ShellFrame>
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
