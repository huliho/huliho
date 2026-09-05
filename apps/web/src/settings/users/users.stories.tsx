// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { ROLES } from "@huliho/core";
import type { UserRow } from "@huliho/core";
import type { Meta, StoryObj } from "@storybook/react-vite";

import { Button } from "../../design-system/button";
import { ListSkeleton } from "../../design-system/list-skeleton";
import { SettingsSection } from "../settings-section";
import type { Flow, UserFlow } from "./use-user-flow";
import { UserDialog } from "./user-dialog";
import { UserTable } from "./user-table";

// Screenshots must not age, so the rows sit at fixed distances from a fixed now.
const NOW = new Date("2026-05-14T10:00:00");
const HOUR_MS = 3_600_000;
const DAY_MS = 24 * HOUR_MS;
// The actor plus two colleagues, the shape a small organization has.
const SKELETON_ROW_COUNT = 3;
const ONE_TIME = "k7fq-2mzp-x4rt";

const JONAS: UserRow = {
  id: "jonas",
  name: "Jonas",
  login: "jonas@example.com",
  role: "member",
  lastActiveAt: NOW.getTime() - DAY_MS,
};

const ROWS: UserRow[] = [
  JONAS,
  {
    id: "mira",
    name: "Mira",
    login: "mira@example.com",
    role: "owner",
    lastActiveAt: NOW.getTime(),
  },
  { id: "noor", name: "Noor", login: "noor@example.com", role: "member", lastActiveAt: null },
  {
    id: "tomas",
    name: "Tomas",
    login: "tomas@example.com",
    role: "admin",
    lastActiveAt: NOW.getTime() - 21 * DAY_MS,
  },
];

function nothing(): void {
  // Stories render states; nothing runs.
}

// A flow frozen on one face of the dialog.
function frozen(flow: Flow, issued: UserFlow["issued"] = null): UserFlow {
  return {
    flow,
    open: true,
    issued,
    pending: false,
    failure: null,
    start: nothing,
    confirmReset: nothing,
    submitCreate: nothing,
    clearFailure: nothing,
    close: nothing,
    settle: nothing,
  };
}

const meta: Meta = {
  title: "Settings/Users",
};

export default meta;

export const Default: StoryObj = {
  render: () => (
    <SettingsSection title="Users" action={<Button variant="primary">Create user</Button>}>
      <UserTable
        rows={ROWS}
        locale="en"
        now={NOW}
        actor={{ id: "mira", role: "owner" }}
        onReset={nothing}
      />
    </SettingsSection>
  ),
};

export const Loading: StoryObj = {
  render: () => (
    <SettingsSection title="Users" action={<Button variant="primary">Create user</Button>}>
      <ListSkeleton locale="en" rows={SKELETON_ROW_COUNT} />
    </SettingsSection>
  ),
};

export const ResetConfirm: StoryObj = {
  render: () => (
    <UserDialog locale="en" roles={[...ROLES]} flow={frozen({ kind: "reset", user: JONAS })} />
  ),
};

export const ResetRefused: StoryObj = {
  render: () => (
    <UserDialog
      locale="en"
      roles={[...ROLES]}
      flow={{ ...frozen({ kind: "reset", user: JONAS }), failure: "unavailable" }}
    />
  ),
};

export const IssuedPassword: StoryObj = {
  render: () => (
    <UserDialog
      locale="en"
      roles={[...ROLES]}
      flow={frozen(
        { kind: "reset", user: JONAS },
        { name: JONAS.name, secret: ONE_TIME, expiresAt: NOW.getTime() + DAY_MS, reason: "reset" },
      )}
    />
  ),
};

export const CreateUser: StoryObj = {
  render: () => <UserDialog locale="en" roles={[...ROLES]} flow={frozen({ kind: "create" })} />,
};
