// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Toast } from "@base-ui/react/toast";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect } from "react";

import { ToastProvider, Toasts } from "./toast";
import type { ToastData } from "./toast";

// A fixed id upserts, so remounting the story never stacks copies.
const STORY_TOAST_ID = "story-undo";
// Zero disables the timer; the screenshot must not race it.
const NO_TIMEOUT = 0;

function noUndo(): void {
  // Stories render states; nothing is undone.
}

// The provider's own add reaches its store at once, before it listens to the manager.
function UndoToast() {
  const { add } = Toast.useToastManager<ToastData>();
  useEffect(() => {
    add({
      id: STORY_TOAST_ID,
      description: "Phone session revoked.",
      timeout: NO_TIMEOUT,
      data: { undo: noUndo },
      actionProps: { onClick: noUndo },
    });
  }, [add]);
  return <Toasts />;
}

const meta: Meta = {
  title: "Primitives/Toast",
};

export default meta;

export const Undo: StoryObj = {
  render: () => (
    <ToastProvider>
      <UndoToast />
    </ToastProvider>
  ),
};
