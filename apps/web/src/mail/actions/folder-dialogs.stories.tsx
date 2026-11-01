// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";
import { useRef } from "react";

import type { Locale } from "../../paraglide/runtime.js";
import type { FolderRefusal, NameAsk } from "./folder-asks";
import { ConfirmDialog, NameForm } from "./folder-dialogs";
import { DIALOG_PARTS } from "./folder-host";
import { managedOf } from "./writable-rig";

function nothing(): void {
  // A drawn dialog writes nothing.
}

// The longest name the drawn account takes, in octets.
const NAME_BYTES = 255;

function ask(kind: NameAsk["kind"], name: string, place: string | null): NameAsk {
  return {
    kind,
    name,
    place,
    taken: ["Facturen", "Verbouwing"],
    limit: NAME_BYTES,
    write: () => Promise.resolve(null),
  };
}

interface DrawnProps {
  locale: Locale;
  ask: NameAsk;
  pending?: boolean;
  refusal?: FolderRefusal | null;
}

function Drawn({ locale, ask: asked, pending = false, refusal = null }: DrawnProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  return (
    <NameForm
      locale={locale}
      ask={asked}
      open
      parts={DIALOG_PARTS}
      pending={pending}
      refusal={refusal}
      inputRef={inputRef}
      onSubmit={nothing}
      onEdit={nothing}
    />
  );
}

const meta: Meta = {
  title: "Mail/FolderDialogs",
};

export default meta;

// A folder at the top, the field empty and holding the focus.
export const NewFolder: StoryObj = {
  render: () => <Drawn locale="en" ask={ask("create", "", null)} />,
};

// A subfolder of Verbouwing, in Dutch.
export const NewSubfolderDutch: StoryObj = {
  render: () => <Drawn locale="nl" ask={ask("create", "", "Verbouwing")} />,
};

// A rename with the name selected.
export const Rename: StoryObj = {
  render: () => <Drawn locale="en" ask={ask("rename", "Offertes", "Verbouwing")} />,
};

// A rename in Dutch.
export const RenameDutch: StoryObj = {
  render: () => <Drawn locale="nl" ask={ask("rename", "Offertes", "Verbouwing")} />,
};

// The create waits for the server's answer.
export const Pending: StoryObj = {
  render: () => <Drawn locale="en" ask={ask("create", "Projecten", null)} pending />,
};

// The name a folder beside it holds, said on the field.
export const Taken: StoryObj = {
  render: () => <Drawn locale="en" ask={ask("create", "Facturen", null)} refusal="taken" />,
};

// A name the server does not take, said in the failure line.
export const Refused: StoryObj = {
  render: () => <Drawn locale="en" ask={ask("create", "2024/2025", null)} refusal="name" />,
};

// The server's refusal of a rename, in Dutch.
export const RefusedDutch: StoryObj = {
  render: () => <Drawn locale="nl" ask={ask("rename", "Offertes", null)} refusal="server" />,
};

// No connection, said in the failure line.
export const Offline: StoryObj = {
  render: () => <Drawn locale="en" ask={ask("create", "Projecten", null)} refusal="offline" />,
};

// The confirm before a folder goes with its five messages.
export const Delete: StoryObj = {
  render: () => (
    <ConfirmDialog
      locale="en"
      ask={{ kind: "delete", mailbox: managedOf("mb-offertes"), confirm: nothing }}
      open
      parts={DIALOG_PARTS}
    />
  ),
};

// The same confirm in Dutch.
export const DeleteDutch: StoryObj = {
  render: () => (
    <ConfirmDialog
      locale="nl"
      ask={{ kind: "delete", mailbox: managedOf("mb-offertes"), confirm: nothing }}
      open
      parts={DIALOG_PARTS}
    />
  ),
};
