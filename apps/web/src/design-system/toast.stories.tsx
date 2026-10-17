// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Toast } from "@base-ui/react/toast";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect } from "react";

import { ToastProvider, Toasts } from "./toast";
import type { ToastData } from "./toast";

// Zero disables the timer; the screenshot must not race it.
const NO_TIMEOUT = 0;

function noUndo(): void {
  // Stories render states; nothing is undone.
}

interface Shown {
  // A fixed id upserts, so remounting the story never stacks copies.
  id: string;
  description: string;
  data: ToastData;
}

// The provider's own add reaches its store at once, before it listens
// to the manager; the toasts land oldest first.
function StoryToasts({ shown }: { shown: readonly Shown[] }) {
  const { add } = Toast.useToastManager<ToastData>();
  useEffect(() => {
    for (const toast of shown) {
      add({
        ...toast,
        timeout: NO_TIMEOUT,
        ...(toast.data.undo === undefined ? {} : { actionProps: { onClick: toast.data.undo } }),
      });
    }
  }, [add, shown]);
  return <Toasts />;
}

function story(shown: readonly Shown[]): StoryObj {
  return {
    render: () => (
      <ToastProvider>
        <StoryToasts shown={shown} />
      </ToastProvider>
    ),
  };
}

const meta: Meta = {
  title: "Primitives/Toast",
};

export default meta;

export const Undo = story([
  { id: "story-undo", description: "Phone session revoked.", data: { undo: noUndo } },
]);

// An archive and then a delete: the newer stands nearest the edge and
// alone carries the key.
export const Stacked = story([
  { id: "story-archived", description: "Archived.", data: { undo: noUndo } },
  { id: "story-deleted", description: "Deleted.", data: { undo: noUndo } },
]);

// A change the server refused, without Undo.
export const Refused = story([
  {
    id: "story-refused",
    description: "Couldn’t archive. Your mail is safe.",
    data: { tone: "danger" },
  },
]);
