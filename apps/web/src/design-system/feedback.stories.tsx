// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";

import { RoutePending } from "../shell/route-fallbacks";
import { BrandMark } from "./brand-mark";
import { ErrorState } from "./error-state";

function noRetry(): void {
  // Stories render states; nothing retries.
}

const meta: Meta = {
  title: "Primitives/Feedback",
};

export default meta;

export const ErrorRecovery: StoryObj = {
  render: () => (
    <ErrorState
      message="Couldn’t reach the server. Check your connection and try again."
      retryLabel="Try again"
      onRetry={noRetry}
    />
  ),
};

export const Pending: StoryObj = {
  render: () => <RoutePending />,
};

export const Brand: StoryObj = {
  render: () => <BrandMark heading />,
};
