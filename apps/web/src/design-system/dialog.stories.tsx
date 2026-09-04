// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";
import { useRef } from "react";

import { Button } from "./button";
import { Dialog } from "./dialog";

function noChange(): void {
  // Stories render states; the dialog stays open.
}

function ConfirmDialog() {
  const cancel = useRef<HTMLButtonElement>(null);
  return (
    <Dialog
      open
      onOpenChange={noChange}
      title="Reset Jonas’s password?"
      description="Jonas gets a one-time password to sign in with, chooses a new one right away and every session of his is signed out."
      initialFocus={cancel}
    >
      <Button ref={cancel}>Cancel</Button>
      <Button variant="danger">Reset password</Button>
    </Dialog>
  );
}

const meta: Meta = {
  title: "Primitives/Dialog",
};

export default meta;

export const Confirm: StoryObj = {
  render: () => <ConfirmDialog />,
};
