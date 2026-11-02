// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { branchOf, hideWrite } from "@huliho/core";
import type { MailCache, Mailbox, MailboxWrite } from "@huliho/core";

import { askFolder } from "./folder-asks";
import type { FolderAction, FolderRefusal, NameAsk } from "./folder-asks";
import type { FolderBase, FolderItemInput } from "./folder-items";
import { folderToast, writeFolder } from "./folder-writes";

function namesUnder(rows: readonly Mailbox[], parentId: string | null, except: string | null) {
  return rows
    .filter((row) => row.parentId === parentId && row.id !== except)
    .map((row) => row.name);
}

// The tree as the store holds it after a write; a read that fails leaves
// its cause in the console and reads as an empty tree.
function treeAfter(cache: MailCache, accountId: string): Promise<Mailbox[]> {
  return cache.mailboxes(accountId).catch((error: unknown): Mailbox[] => {
    console.error(
      "mail: a folder tree read failed",
      error instanceof Error ? error.message : String(error),
    );
    return [];
  });
}

// What the name dialog writes: a new folder under `parentId`, or the new
// name of `renamed`.
interface NameTarget {
  parentId: string | null;
  renamed: Mailbox | null;
}

// A name the server refused is taken where the tree that write left holds
// it beside the folder, read in NFC as the dialog sends the name;
// otherwise the server does not take it.
async function refusedName(base: FolderBase, target: NameTarget, name: string) {
  const rows = await treeAfter(base.cache, base.accountId);
  const taken = namesUnder(rows, target.parentId, target.renamed?.id ?? null).some(
    (row) => row.normalize("NFC") === name,
  );
  return taken ? "taken" : "name";
}

// A new folder keeps the account's way and says so, since servers differ
// on the default. Where the account subscribes a mailbox it is made
// subscribed, so the tree draws it; where it subscribes none it is made
// unsubscribed, so the tree goes on drawing every mailbox.
function createOf(base: FolderBase, parentId: string | null): (name: string) => MailboxWrite {
  const isSubscribed = base.mailboxes.some((row) => row.isSubscribed);
  return (name) => ({ create: { folder: { name, parentId, isSubscribed } } });
}

function nameAsk(base: FolderBase, target: NameTarget): NameAsk {
  const { parentId, renamed } = target;
  const write: (name: string) => MailboxWrite =
    renamed === null
      ? createOf(base, parentId)
      : (name) => ({ update: { [renamed.id]: { name } } });
  return {
    kind: renamed === null ? "create" : "rename",
    name: renamed?.name ?? "",
    place: base.mailboxes.find((row) => row.id === parentId)?.name ?? null,
    taken: namesUnder(base.mailboxes, parentId, renamed?.id ?? null),
    limit: base.limits.maxSizeMailboxName,
    write: async (name): Promise<FolderRefusal | null> => {
      if (name === renamed?.name.normalize("NFC")) {
        return null;
      }
      const refusal = await writeFolder(base.cache, base.accountId, write(name));
      if (refusal === null) {
        folderToast(renamed === null ? "created" : "renamed", null);
      }
      return refusal === "name" ? refusedName(base, target, name) : refusal;
    },
  };
}

// Opens the name dialog for a new folder under `parent`, null for the top.
export function newFolder(base: FolderBase, parent: Mailbox | null): void {
  askFolder(nameAsk(base, { parentId: parent?.id ?? null, renamed: null }));
}

// The mailbox shown leaves first wherever it stands in the branch. The
// write unsubscribes the subscribed folders of the branch; a folder drawn
// without a subscription of its own stands above one, so there is one.
async function hide(input: FolderItemInput): Promise<void> {
  if (!navigator.onLine) {
    folderToast("hidden", "offline");
    return;
  }
  const branch = branchOf(input.mailboxes, input.mailbox.id);
  if (input.shownId !== undefined && branch.includes(input.shownId)) {
    input.leave();
  }
  const write = hideWrite(input.mailboxes, input.mailbox.id);
  folderToast("hidden", await writeFolder(input.cache, input.accountId, write));
}

// A destroy leaves the mailbox shown first. A folder that took mail since
// the tree was read is refused, so the confirm asks with its count as
// the store now holds it.
async function destroy(input: FolderItemInput, withMail: boolean): Promise<void> {
  const { cache, accountId, mailbox } = input;
  if (input.shownId === mailbox.id) {
    input.leave();
  }
  const write = { destroy: [mailbox.id], onDestroyRemoveEmails: withMail };
  const refusal = await writeFolder(cache, accountId, write);
  const fresh =
    refusal === "mail"
      ? (await treeAfter(cache, accountId)).find((row) => row.id === mailbox.id)
      : undefined;
  if (fresh === undefined) {
    folderToast("deleted", refusal);
  } else {
    askDelete({ ...input, mailbox: fresh });
  }
}

function askDelete(input: FolderItemInput): void {
  askFolder({
    kind: "delete",
    mailbox: input.mailbox,
    confirm: () => {
      void destroy(input, true);
    },
  });
}

// A delete: refused at once for a folder with subfolders or offline; a
// folder with mail asks the confirm first.
function remove(input: FolderItemInput): void {
  if (input.mailboxes.some((row) => row.parentId === input.mailbox.id)) {
    folderToast("deleted", "children");
  } else if (!navigator.onLine) {
    folderToast("deleted", "offline");
  } else if (input.mailbox.totalEmails > 0) {
    askDelete(input);
  } else {
    void destroy(input, false);
  }
}

// What a folder item does once it is pressed.
export function runFolder(action: FolderAction, input: FolderItemInput): void {
  switch (action) {
    case "subfolder":
      newFolder(input, input.mailbox);
      break;
    case "rename":
      askFolder(nameAsk(input, { parentId: input.mailbox.parentId, renamed: input.mailbox }));
      break;
    case "hide":
      void hide(input);
      break;
    default:
      remove(input);
  }
}
