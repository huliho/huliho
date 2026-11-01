// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { visibleMailboxes } from "@huliho/core";
import type { MailCache, Mailbox, MailboxLimits } from "@huliho/core";

import type { Chord } from "../../commands/keys";
import type { Command } from "../../commands/registry";
import { useCommand } from "../../commands/use-command";
import { toastManager } from "../../design-system/toast";
import { m } from "../../paraglide/messages.js";
import { getLocale } from "../../paraglide/runtime.js";
import type { Locale } from "../../paraglide/runtime.js";
import { prefetchMenus } from "../menus-chunk";
import type { FolderAction } from "./folder-asks";
import type { FolderItem } from "./folder-jobs";

// What every folder write of an account takes: its cache, every mailbox
// it holds and its folder limits.
export interface FolderBase {
  cache: MailCache;
  accountId: string;
  mailboxes: readonly Mailbox[];
  limits: MailboxLimits;
}

// What the shell hands the folder items, once the account's limits are
// in: those limits and the way to its inbox.
export interface FolderShell {
  limits: MailboxLimits;
  leave: () => void;
}

// One mailbox's folder items: the mailbox, the screen's words and the way
// to the inbox, which a delete or a hide of the mailbox shown takes first.
export interface FolderItemInput extends FolderBase {
  locale: Locale;
  mailbox: Mailbox;
  shownId: string | undefined;
  leave: () => void;
}

// The folder actions reach the palette without a key.
const NO_KEYS: readonly Chord[] = [];

const ACTIONS: readonly FolderAction[] = ["subfolder", "rename", "hide", "delete"];

// The actions a mailbox offers: a subfolder where it takes one, a new
// name where it may take one and never on INBOX, a hide on a folder
// without a role that the tree draws where the account keeps
// subscriptions, a delete on a folder without a role where it may be
// deleted.
function offered({ mailbox, mailboxes }: FolderItemInput): FolderAction[] {
  const { myRights: rights, role } = mailbox;
  const allowed = new Map<FolderAction, boolean>([
    ["subfolder", rights.mayCreateChild],
    ["rename", rights.mayRename && role !== "inbox"],
    [
      "hide",
      role === null &&
        mailboxes.some((row) => row.isSubscribed) &&
        visibleMailboxes(mailboxes).some((row) => row.id === mailbox.id),
    ],
    ["delete", rights.mayDelete && role === null],
  ]);
  return ACTIONS.filter((action) => allowed.get(action) === true);
}

type Menus = Awaited<ReturnType<typeof prefetchMenus>>;

// The folder actions run from the menus chunk, which holds their dialogs
// and their writes; a chunk that does not come says so in a toast.
function fromMenus(run: (menus: Menus) => void): void {
  prefetchMenus()
    .then(run)
    .catch((error: unknown) => {
      console.error(
        "mail: a folder action did not load",
        error instanceof Error ? error.message : String(error),
      );
      toastManager.add({
        description: m.pane_error({}, { locale: getLocale() }),
        data: { tone: "danger" },
      });
    });
}

// Opens the name dialog for a new folder under `parent`, null for the top.
export function askNewFolder(base: FolderBase, parent: Mailbox | null): void {
  fromMenus((menus) => {
    menus.newFolder(base, parent);
  });
}

function runOf(input: FolderItemInput, action: FolderAction): () => void {
  return () => {
    fromMenus((menus) => {
      menus.runFolder(action, input);
    });
  };
}

function labelOf(action: FolderAction, locale: Locale): string {
  switch (action) {
    case "subfolder":
      return m.folder_new_subfolder({}, { locale });
    case "rename":
      return m.folder_rename({}, { locale });
    case "hide":
      return m.folder_hide({}, { locale });
    default:
      return m.folder_delete({}, { locale });
  }
}

// Every item of a mailbox's menu in its order: Mark all as read, the
// folder items, then the items that empty.
export function withFolderItems(jobs: FolderItem[], folders: FolderItem[]): FolderItem[] {
  return [
    ...jobs.filter((item) => !item.danger),
    ...folders,
    ...jobs.filter((item) => item.danger),
  ];
}

// The folder items of a mailbox's menu, the delete in danger.
export function folderItems(input: FolderItemInput): FolderItem[] {
  return offered(input).map((action) => ({
    id: action,
    label: labelOf(action, input.locale),
    danger: action === "delete",
    run: runOf(input, action),
  }));
}

function commandOf(
  items: readonly FolderItem[],
  action: FolderAction,
  locale: Locale,
): Command | null {
  const item = items.find((entry) => entry.id === action);
  return item === undefined
    ? null
    : {
        id: `folder.${action}`,
        label: m.command_folder({ kind: action }, { locale }),
        group: "act",
        keys: NO_KEYS,
        run: item.run,
      };
}

// Every folder item of the mailbox shown is in the palette, worded as an
// act on a folder; New folder stands there while the account takes one
// at the top.
export function useFolderItemCommands(
  items: readonly FolderItem[],
  newFolder: (() => void) | null,
  locale: Locale,
): void {
  useCommand(
    newFolder === null
      ? null
      : {
          id: "folder.new",
          label: m.command_folder({ kind: "new" }, { locale }),
          group: "act",
          keys: NO_KEYS,
          run: newFolder,
        },
  );
  useCommand(commandOf(items, "subfolder", locale));
  useCommand(commandOf(items, "rename", locale));
  useCommand(commandOf(items, "hide", locale));
  useCommand(commandOf(items, "delete", locale));
}
