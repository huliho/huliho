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

function nothing(): void {
  // Stories render states; an action changes none of them.
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
      const action = toast.data.undo ?? toast.data.retry;
      add({
        ...toast,
        timeout: NO_TIMEOUT,
        ...(action === undefined ? {} : { actionProps: { onClick: action } }),
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
  { id: "story-undo", description: "Phone session revoked.", data: { undo: nothing } },
]);

// An archive and then a delete: the newer stands nearest the edge and
// alone carries the key.
export const Stacked = story([
  { id: "story-archived", description: "Archived.", data: { undo: nothing } },
  { id: "story-deleted", description: "Deleted.", data: { undo: nothing } },
]);

// A change the server refused, without Undo.
export const Refused = story([
  {
    id: "story-refused",
    description: "Couldn’t archive. Your mail is safe.",
    data: { tone: "danger" },
  },
]);

// A folder job that failed part way, with Try again.
export const Failed = story([
  {
    id: "story-failed",
    description: "Couldn’t empty Trash. 400 of 1,204 were deleted.",
    data: { tone: "danger", retry: nothing },
  },
]);
