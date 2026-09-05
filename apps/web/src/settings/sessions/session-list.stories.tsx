// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { SessionRow } from "@huliho/core";
import type { Meta, StoryObj } from "@storybook/react-vite";

import { ListSkeleton } from "../../design-system/list-skeleton";
import { SettingsSection } from "../settings-section";
import { SessionList } from "./session-list";

// Screenshots must not age, so the rows sit at fixed distances from a fixed now.
const NOW = new Date("2026-05-14T10:00:00");
const HOUR_MS = 3_600_000;
const DAY_MS = 24 * HOUR_MS;
// The current device plus two typical others, the shape the list usually has.
const SKELETON_ROW_COUNT = 3;

const ROWS: SessionRow[] = [
  {
    id: "current",
    current: true,
    device: { browser: "Firefox", os: "Linux", phone: false, installed: false },
    address: "203.0.113.7",
    createdAt: NOW.getTime() - DAY_MS,
    lastSeenAt: NOW.getTime(),
  },
  {
    id: "phone",
    current: false,
    device: { browser: "Chrome", os: "Android", phone: true, installed: true },
    address: "203.0.113.7",
    createdAt: NOW.getTime() - 3 * DAY_MS,
    lastSeenAt: NOW.getTime() - 2 * HOUR_MS,
  },
  {
    id: "mac",
    current: false,
    device: { browser: "Safari", os: "macOS", phone: false, installed: false },
    address: "198.51.100.23",
    createdAt: NOW.getTime() - 30 * DAY_MS,
    lastSeenAt: NOW.getTime() - 21 * DAY_MS,
  },
];

function nothing(): void {
  // Stories render states; nothing is revoked.
}

const meta: Meta = {
  title: "Settings/Sessions",
};

export default meta;

export const Default: StoryObj = {
  render: () => (
    <SettingsSection title="Active sessions">
      <SessionList rows={ROWS} locale="en" now={NOW} onRevoke={nothing} onRevokeOthers={nothing} />
    </SettingsSection>
  ),
};

export const Loading: StoryObj = {
  render: () => (
    <SettingsSection title="Active sessions">
      <ListSkeleton locale="en" rows={SKELETON_ROW_COUNT} />
    </SettingsSection>
  ),
};
