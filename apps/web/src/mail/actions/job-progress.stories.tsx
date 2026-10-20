// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Job, JobProgress } from "@huliho/core";
import type { Meta, StoryObj } from "@storybook/react-vite";
import type { ReactNode } from "react";

import { fixtureCache } from "../fixture-cache";
import header from "../pane-header.module.css";
import { JobProgressBar, OfflineAsk } from "./job-progress";
import { boxOf } from "./writable-rig";

// The pane header the progress takes the place of the name in.
function Header({ children }: { children: ReactNode }) {
  return <header className={header.header}>{children}</header>;
}

function running(job: Job, done: number, total: number | null): JobProgress {
  return { job, done, total, last: null, state: "running" };
}

const TRASH = boxOf("trash");
const INBOX = boxOf("inbox");
const EMPTY: Job = { kind: "empty", mailboxId: TRASH.id };
const MARK_READ: Job = { kind: "markRead", mailboxId: INBOX.id };

function bar(progress: JobProgress, mailbox = TRASH): StoryObj {
  return {
    render: () => (
      <Header>
        <JobProgressBar
          locale="en"
          cache={fixtureCache({})}
          accountId="acc-1"
          progress={progress}
          mailbox={mailbox}
        />
      </Header>
    ),
  };
}

const meta: Meta = {
  title: "Mail/JobProgress",
};

export default meta;

// Trash emptying, a round in: the count and the hairline at done over total.
export const Emptying = bar(running(EMPTY, 500, 1204));

// Marking the inbox read, a quarter through.
export const Marking = bar(running(MARK_READ, 1000, 4000), INBOX);

// Before the first round has counted the job: the sentence alone.
export const Starting = bar(running(EMPTY, 0, null));

// A job asked offline: the answer in the same place.
export const Offline: StoryObj = {
  render: () => (
    <Header>
      <OfflineAsk locale="en" />
    </Header>
  ),
};
