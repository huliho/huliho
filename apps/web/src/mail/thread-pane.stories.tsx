// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { JmapError } from "@huliho/core";
import type { Meta, StoryObj } from "@storybook/react-vite";
import type { JSX } from "react";

import {
  FASTMAIL,
  INBOX_ID,
  MAILBOXES,
  THREAD,
  THREAD_ID,
  fixtureCache,
  threadDetail,
} from "./fixtures";
import type { ThreadAnswer } from "./fixtures";
import { ReadingPane, ThreadScreen, prefetchThreadPane } from "./reading-pane";
import type { OpenThread } from "./reading-pane";
import { routed } from "./story-router";

const INBOX = MAILBOXES.find((mailbox) => mailbox.id === INBOX_ID);
const OPEN: OpenThread = { accountId: FASTMAIL.id, threadId: THREAD_ID, mailbox: INBOX };
// An older message left unread, which the pane opens in sight.
const UNREAD_AT = 4;

function nothing(): void {
  // A drawn pane closes nothing.
}

// The pane's box beside the list, so the cards have a height to scroll in.
function Box({ children }: { children: JSX.Element }): JSX.Element {
  return <div style={{ position: "relative", display: "flex", blockSize: "80vh" }}>{children}</div>;
}

function paned(answer: ThreadAnswer): JSX.Element {
  return routed(() => (
    <Box>
      <ReadingPane
        locale="en"
        cache={fixtureCache({}, { [THREAD_ID]: answer })}
        thread={OPEN}
        position="right"
        keyHints
        onClose={nothing}
      />
    </Box>
  ));
}

// The pane comes as a chunk of its own; every story waits for it before
// it renders, so no screenshot shows the still cards in its place.
const meta: Meta = {
  title: "Mail/ThreadPane",
  loaders: [
    async () => {
      await prefetchThreadPane();
      return {};
    },
  ],
};

export default meta;

export const Default: StoryObj = {
  render: () => paned(THREAD),
};

export const UnreadInside: StoryObj = {
  render: () => paned(threadDetail([UNREAD_AT])),
};

export const Screen: StoryObj = {
  render: () =>
    routed(() => (
      <Box>
        <ThreadScreen
          locale="en"
          cache={fixtureCache({}, { [THREAD_ID]: THREAD })}
          thread={OPEN}
          keyHints
          onClose={nothing}
        />
      </Box>
    )),
};

export const Loading: StoryObj = {
  render: () => paned("never"),
};

export const Failed: StoryObj = {
  render: () => paned(new JmapError("unavailable")),
};

export const Gone: StoryObj = {
  render: () => paned(null),
};
