// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "@huliho/core";
import { useSyncExternalStore } from "react";

import { awaitLayer } from "../../commands/registry";

// What a folder's menu offers beside its jobs.
export type FolderAction = "subfolder" | "rename" | "hide" | "delete";

// Why a folder write did not land, as the dialogs and the toasts word
// it: a name that cannot go out as typed, a refusal of the server's, a
// folder that still holds subfolders or mail, no connection.
export type FolderRefusal =
  "empty" | "long" | "taken" | "name" | "children" | "mail" | "offline" | "server";

// The name dialog: a new folder under a parent or at the top, or a new
// name for a folder. `place` is the parent's name, null for the top;
// `taken` the names beside it; `write` sends the name and answers why
// it did not land, null once it did.
export interface NameAsk {
  kind: "create" | "rename";
  name: string;
  place: string | null;
  taken: readonly string[];
  // The longest name the account takes, in octets of UTF-8.
  limit: number;
  write: (name: string) => Promise<FolderRefusal | null>;
}

// The confirm before a destroy: emptying Trash or Junk, or deleting a
// folder with its mail. `confirm` runs once the user says so.
export interface ConfirmAsk {
  kind: "empty" | "delete";
  mailbox: Mailbox;
  confirm: () => void;
}

export type FolderAsk = NameAsk | ConfirmAsk;

// The ask stays through the closing fade, so the words stay put; `seq`
// tells one ask from the next.
export interface Asked {
  ask: FolderAsk | null;
  open: boolean;
  seq: number;
}

let asked: Asked = { ask: null, open: false, seq: 0 };
const listeners = new Set<() => void>();

function publish(next: Asked): void {
  asked = next;
  for (const listener of listeners) {
    listener();
  }
}

// Opens the dialog the menus and the palette ask for; the keys wait for it.
export function askFolder(ask: FolderAsk): void {
  awaitLayer();
  publish({ ask, open: true, seq: asked.seq + 1 });
}

export function closeFolderAsk(): void {
  publish({ ...asked, open: false });
}

// The closing fade has ended, so the dialog leaves; a dialog asked
// meanwhile stays.
export function settleFolderAsk(): void {
  if (!asked.open) {
    publish({ ...asked, ask: null });
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function read(): Asked {
  return asked;
}

export function useFolderAsk(): Asked {
  return useSyncExternalStore(subscribe, read, read);
}
