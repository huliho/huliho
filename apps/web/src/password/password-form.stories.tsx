// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";

import { SettingsSection } from "../settings/settings-section";
import { PasswordForm } from "./password-form";

const RETRY_SECONDS = 872;

function noSubmit(): void {
  // Stories render states; nothing submits.
}

const meta: Meta = {
  title: "Settings/Password",
};

export default meta;

export const Default: StoryObj = {
  render: () => (
    <SettingsSection title="Password">
      <PasswordForm
        locale="en"
        mode="change"
        pending={false}
        failure={null}
        retryRemaining={null}
        onSubmit={noSubmit}
      />
    </SettingsSection>
  ),
};

export const Saving: StoryObj = {
  render: () => (
    <SettingsSection title="Password">
      <PasswordForm
        locale="en"
        mode="change"
        pending
        failure={null}
        retryRemaining={null}
        onSubmit={noSubmit}
      />
    </SettingsSection>
  ),
};

export const WrongCurrent: StoryObj = {
  render: () => (
    <SettingsSection title="Password">
      <PasswordForm
        locale="en"
        mode="change"
        pending={false}
        failure="invalid_credentials"
        retryRemaining={null}
        onSubmit={noSubmit}
      />
    </SettingsSection>
  ),
};

export const ServerUnreachable: StoryObj = {
  render: () => (
    <SettingsSection title="Password">
      <PasswordForm
        locale="en"
        mode="change"
        pending={false}
        failure="unavailable"
        retryRemaining={null}
        onSubmit={noSubmit}
      />
    </SettingsSection>
  ),
};

export const TooManyAttempts: StoryObj = {
  render: () => (
    <SettingsSection title="Password">
      <PasswordForm
        locale="en"
        mode="change"
        pending={false}
        failure="rate_limited"
        retryRemaining={RETRY_SECONDS}
        onSubmit={noSubmit}
      />
    </SettingsSection>
  ),
};
