// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";
import type { JSX } from "react";

import {
  FASTMAIL,
  FIXED_NOW,
  INBOX_ID,
  INBOX_PAGE,
  MAILBOXES,
  MAILBOXES_EMPTY_INBOX,
  THREAD,
  THREAD_ID,
  fixtureCache,
} from "./fixtures";
import type { PageAnswer } from "./fixtures";
import { EmptyMailbox } from "./mailbox-pane";
import { startOfDay } from "./row-time";
import { ThreadList } from "./thread-list";

const TODAY = startOfDay(FIXED_NOW);
const INBOX = MAILBOXES.find((mailbox) => mailbox.id === INBOX_ID) ?? MAILBOXES[0];

export interface FixtureListProps {
  mailbox?: Mailbox | undefined;
  page?: PageAnswer;
  online?: boolean;
  openThreadId?: string | null;
}

function nothing(): void {
  // A drawn list opens nothing.
}

// The thread list over the fixtures, as the stories draw it.
export function FixtureList(props: FixtureListProps): JSX.Element {
  const { mailbox = INBOX, page = INBOX_PAGE, online = true, openThreadId = null } = props;
  if (mailbox === undefined) {
    return <p>no inbox in the fixtures</p>;
  }
  return (
    <ThreadList
      locale="en"
      today={TODAY}
      cache={fixtureCache({ [`${mailbox.id}/0`]: page }, { [THREAD_ID]: THREAD })}
      accountId={FASTMAIL.id}
      mailbox={mailbox}
      online={online}
      openThreadId={openThreadId}
      empty={
        <EmptyMailbox
          locale="en"
          accountId={FASTMAIL.id}
          mailbox={mailbox}
          mailboxes={MAILBOXES_EMPTY_INBOX}
        />
      }
      onOpen={nothing}
    />
  );
}
