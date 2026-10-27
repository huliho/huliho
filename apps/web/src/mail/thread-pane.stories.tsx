// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { JmapError } from "@huliho/core";
import type { BodyDetail, MemberState, ThreadDetail } from "@huliho/core";
import type { Meta, StoryObj } from "@storybook/react-vite";
import type { JSX } from "react";

import { placesOf } from "./actions/taken";
import { WRITABLE } from "./actions/writable-rig";
import { NEWSLETTER_HTML, PASSED, REPLY_TEXT, htmlDetail, textDetail } from "./body-fixtures";
import { fixtureCache } from "./fixture-cache";
import type { ThreadAnswer } from "./fixture-cache";
import { FASTMAIL, INBOX_ID, THREAD, THREAD_ID, threadDetail } from "./fixtures";
import { ReadingPane, ThreadScreen, prefetchThreadPane } from "./reading-pane";
import type { OpenThread } from "./reading-pane";
import { routed, seedMail } from "./story-router";

// The thread opened from a mailbox; `acting` gives it the offer of the
// writable tree without the move, which these stories leave to the keys,
// and without it the toolbar holds Close alone.
function openFrom(mailboxId: string, acting = false): OpenThread {
  const place = placesOf(WRITABLE)(mailboxId);
  const offered = place === null ? null : { ...place.offered, move: [] };
  return {
    accountId: FASTMAIL.id,
    threadId: THREAD_ID,
    place: place === null || offered === null || !acting ? null : { ...place, offered },
    back: place === null ? { kind: "unnamed" } : { kind: "mailbox", name: place.mailbox.name },
    path: `/mail/${FASTMAIL.id}/${mailboxId}/${THREAD_ID}`,
  };
}

const OPEN = openFrom(INBOX_ID);
const OPEN_ACTING = openFrom(INBOX_ID, true);
// An older message left unread, which the pane opens in sight.
const UNREAD_AT = 4;
const NEWEST_ID = "e-3";
const UNREAD_ID = "e-3-m5";

// The newest message as a newsletter and the unread one as a quoted
// reply; every other card shows its preview.
const BODIES = new Map<string, BodyDetail>([
  [NEWEST_ID, htmlDetail(NEWEST_ID, NEWSLETTER_HTML, { authentication: PASSED })],
  [UNREAD_ID, textDetail(UNREAD_ID, REPLY_TEXT, { flowed: { delSp: false } })],
]);

function nothing(): void {
  // A drawn pane closes nothing and acts on nothing.
}

// Trash in the fixture tree, where the toolbar stands without archive.
const TRASH_ID = "mb-trash";

// The fixture thread with every message moved to Trash.
function inTrash(detail: ThreadDetail): ThreadDetail {
  const members = Object.entries(detail.thread.members).map(
    ([id, member]): [string, MemberState] => [id, { ...member, mailboxIds: { [TRASH_ID]: true } }],
  );
  return { ...detail, thread: { ...detail.thread, members: Object.fromEntries(members) } };
}

// The pane's box beside the list, so the cards have a height to scroll in.
function Box({ children }: { children: JSX.Element }): JSX.Element {
  return <div style={{ position: "relative", display: "flex", blockSize: "80vh" }}>{children}</div>;
}

function paned(answer: ThreadAnswer, open = OPEN): JSX.Element {
  return routed(
    () => (
      <Box>
        <ReadingPane
          locale="en"
          cache={fixtureCache({}, { [THREAD_ID]: answer })}
          thread={open}
          position="right"
          keyHints
          onClose={nothing}
        />
      </Box>
    ),
    undefined,
    seedMail(BODIES),
  );
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
    routed(
      () => (
        <Box>
          <ThreadScreen
            locale="en"
            cache={fixtureCache({}, { [THREAD_ID]: THREAD })}
            thread={OPEN}
            keyHints
            onClose={nothing}
          />
        </Box>
      ),
      undefined,
      seedMail(BODIES),
    ),
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

// The toolbar in the inbox: archive, delete and spam with their words
// where the pane has the room, the two flips as icons.
export const Toolbar: StoryObj = {
  render: () => paned(threadDetail([UNREAD_AT]), OPEN_ACTING),
};

// In Trash delete forever stands where delete would and archive is absent.
export const ToolbarInTrash: StoryObj = {
  render: () => paned(inTrash(THREAD), openFrom(TRASH_ID, true)),
};

// As a screen the actions follow the way back and its key.
export const ScreenToolbar: StoryObj = {
  render: () =>
    routed(
      () => (
        <Box>
          <ThreadScreen
            locale="en"
            cache={fixtureCache({}, { [THREAD_ID]: THREAD })}
            thread={OPEN_ACTING}
            keyHints
            onClose={nothing}
          />
        </Box>
      ),
      undefined,
      seedMail(BODIES),
    ),
};
