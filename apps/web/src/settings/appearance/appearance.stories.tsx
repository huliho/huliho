// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";

import { AppearanceForm } from "./appearance-form";
import { AppearanceSkeleton } from "./appearance-page";

function nothing(): void {
  // Stories render states; nothing is saved.
}

const meta: Meta = {
  title: "Settings/Appearance",
};

export default meta;

export const Default: StoryObj = {
  render: () => (
    <AppearanceForm
      locale="en"
      preferences={{ theme: "dark", density: "compact", readingPane: "bottom", locale: "en" }}
      onChange={nothing}
      onSwitchLocale={nothing}
    />
  ),
};

export const Loading: StoryObj = {
  render: () => <AppearanceSkeleton locale="en" />,
};
