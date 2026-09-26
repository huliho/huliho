// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow } from "@huliho/core";
import type { Meta, StoryObj } from "@storybook/react-vite";
import type { JSX } from "react";

import type { RetryOutcome } from "../accounts/use-retry-account";
import { EXPIRED, PROBE_INTERVAL_MINUTES, STOPPED } from "./fixtures";
import { routed } from "./story-router";
import { ThreadListBanner } from "./thread-list-banner";

function nothing(): void {
  // A drawn banner retries nothing.
}

// The banner at the width of the list beside the reading pane.
function drawn(account: AccountRow, outcome?: RetryOutcome): JSX.Element {
  return routed(() => (
    <div style={{ inlineSize: "var(--hhx-mail-list-width)" }}>
      <ThreadListBanner
        locale="en"
        account={account}
        probeIntervalMinutes={PROBE_INTERVAL_MINUTES}
        online
        outcome={outcome}
        onRetry={nothing}
        takeFocus={false}
        onFocusTaken={nothing}
      />
    </div>
  ));
}

const meta: Meta = {
  title: "Mail/ThreadListBanner",
};

export default meta;

export const Expired: StoryObj = {
  render: () => drawn(EXPIRED),
};

export const Stopped: StoryObj = {
  render: () => drawn(STOPPED),
};

export const Retrying: StoryObj = {
  render: () => drawn(STOPPED, "pending"),
};

export const StillStopped: StoryObj = {
  render: () => drawn(STOPPED, "stillStopped"),
};

export const RetryFailed: StoryObj = {
  render: () => drawn(STOPPED, "failed"),
};
