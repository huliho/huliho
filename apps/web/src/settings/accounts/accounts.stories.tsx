// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow } from "@huliho/core";
import type { Meta, StoryObj } from "@storybook/react-vite";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from "@tanstack/react-router";
import type { JSX } from "react";

import type { RetryOutcomes } from "../../accounts/use-retry-account";
import { cx } from "../../design-system/cx";
import { EmptyState } from "../../design-system/empty-state";
import { ListSkeleton } from "../../design-system/list-skeleton";
import { SettingsSection } from "../settings-section";
import { AccountList } from "./account-list";
import { AddAccountLink } from "./accounts-page";
import styles from "./accounts-page.module.css";

// Screenshots must not age, so the rows sit at fixed distances from a fixed now.
const NOW = new Date("2026-05-14T10:00:00").getTime();
const HOUR_MS = 3_600_000;
const DAY_MS = 24 * HOUR_MS;
// The server's default, so the sentence renders with it.
const PROBE_INTERVAL_MINUTES = 15;
// One or two accounts, the shape a personal list has.
const SKELETON_ROW_COUNT = 2;

const CONNECTED: AccountRow = {
  id: "acc-1",
  address: "sanne@fastmail.com",
  name: "Fastmail",
  provider: "fastmail",
  kind: "jmap",
  authMethod: "bearer",
  stoppedCause: null,
  stoppedAt: null,
  createdAt: NOW - 30 * DAY_MS,
};
const EXPIRED: AccountRow = {
  id: "acc-2",
  address: "s.bakker@gmail.com",
  name: "Gmail",
  provider: "gmail",
  kind: "imap",
  authMethod: "oauth2",
  stoppedCause: "credentials",
  stoppedAt: NOW - DAY_MS,
  createdAt: NOW - 20 * DAY_MS,
};
const STOPPED: AccountRow = {
  id: "acc-3",
  address: "sanne@dekker-mail.nl",
  name: "dekker-mail.nl",
  provider: "generic",
  kind: "imap",
  authMethod: "password",
  stoppedCause: "connection",
  stoppedAt: NOW - HOUR_MS,
  createdAt: NOW - 10 * DAY_MS,
};
const ROWS = [CONNECTED, EXPIRED, STOPPED];
const RESUMED_ROWS = [CONNECTED, EXPIRED, { ...STOPPED, stoppedCause: null, stoppedAt: null }];
const NO_OUTCOMES: RetryOutcomes = {};

function nothing(): void {
  // Stories render states; nothing runs.
}

// The links need a router; a memory one at the page's own address serves.
function routed(Screen: () => JSX.Element): JSX.Element {
  const router = createRouter({
    routeTree: createRootRoute({ component: Screen }),
    history: createMemoryHistory({ initialEntries: ["/settings/accounts"] }),
  });
  return <RouterProvider router={router} />;
}

interface PageProps {
  rows: AccountRow[];
  outcomes?: RetryOutcomes;
}

function Page({ rows, outcomes = NO_OUTCOMES }: PageProps): JSX.Element {
  return (
    <SettingsSection title="Mail accounts">
      {rows.length === 0 ? (
        <EmptyState message="Your mail stays at your provider. This is where you read it." />
      ) : (
        <AccountList
          rows={rows}
          locale="en"
          probeIntervalMinutes={PROBE_INTERVAL_MINUTES}
          outcomes={outcomes}
          onRetry={nothing}
          onRemove={nothing}
          afterLast={{ current: null }}
        />
      )}
      <div className={cx(styles.footer, rows.length === 0 ? styles.centered : undefined)}>
        <AddAccountLink locale="en" />
      </div>
    </SettingsSection>
  );
}

const meta: Meta = {
  title: "Settings/Accounts",
};

export default meta;

export const Default: StoryObj = {
  render: () => routed(() => <Page rows={ROWS} />),
};

export const Loading: StoryObj = {
  render: () => (
    <SettingsSection title="Mail accounts">
      <ListSkeleton locale="en" rows={SKELETON_ROW_COUNT} />
    </SettingsSection>
  ),
};

export const Empty: StoryObj = {
  render: () => routed(() => <Page rows={[]} />),
};

export const Retrying: StoryObj = {
  render: () => routed(() => <Page rows={ROWS} outcomes={{ "acc-3": "pending" }} />),
};

export const StillStopped: StoryObj = {
  render: () => routed(() => <Page rows={ROWS} outcomes={{ "acc-3": "stillStopped" }} />),
};

export const Resumed: StoryObj = {
  render: () => routed(() => <Page rows={RESUMED_ROWS} outcomes={{ "acc-3": "resumed" }} />),
};
