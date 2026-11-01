// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MailCache, MailboxRefusal, MailboxWrite } from "@huliho/core";

import { toastManager } from "../../design-system/toast";
import { m } from "../../paraglide/messages.js";
import { getLocale } from "../../paraglide/runtime.js";
import type { FolderRefusal } from "./folder-asks";

// What a toast says once a write landed.
export type FolderDone = "created" | "renamed" | "deleted" | "hidden";

// A refusal of the server's as the user reads it: a name it does not
// take, a folder that still holds subfolders or mail, anything else.
function refusalOf({ type, properties }: MailboxRefusal): FolderRefusal {
  switch (type) {
    case "invalidProperties":
      return properties.includes("name") ? "name" : "server";
    case "mailboxHasChild":
      return "children";
    case "mailboxHasEmail":
      return "mail";
    default:
      return "server";
  }
}

// Sends one folder write; answers why it did not land, null once it did.
// A write refused whole, or one that never reached the worker, reads as
// the server's refusal; its cause stays in the console.
export async function writeFolder(
  cache: MailCache,
  accountId: string,
  write: MailboxWrite,
): Promise<FolderRefusal | null> {
  try {
    const outcome = await cache.setMailbox(accountId, write);
    const refused = [
      ...Object.values(outcome.notCreated),
      ...Object.values(outcome.notUpdated),
      ...Object.values(outcome.notDestroyed),
    ].at(0);
    return refused === undefined ? null : refusalOf(refused);
  } catch (error) {
    console.error(
      "mail: a folder write failed",
      error instanceof Error ? error.message : String(error),
    );
    return "server";
  }
}

// The toast a write ends with: what landed, or why it did not in the
// danger tone.
export function folderToast(done: FolderDone, refusal: FolderRefusal | null): void {
  const locale = getLocale();
  if (refusal === null) {
    toastManager.add({ description: m.folder_done({ kind: done }, { locale }) });
    return;
  }
  toastManager.add({
    description: m.folder_refused({ reason: refusal }, { locale }),
    data: { tone: "danger" },
  });
}
