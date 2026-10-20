// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { offeredJobs } from "@huliho/core";
import type { Job, JobProgress, MailCache, Mailbox } from "@huliho/core";

import type { Chord } from "../../commands/keys";
import type { Command } from "../../commands/registry";
import { useCommand } from "../../commands/use-command";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { askEmpty } from "./confirm-empty";
import { askedOffline, heardJob } from "./job-store";

// One item of a mailbox's menu as the shell words it for the menus
// chunk; the jobs that empty stand under a rule in danger.
export interface FolderItem {
  id: "select" | Job["kind"];
  label: string;
  danger: boolean;
  run: () => void;
}

export interface FolderJobInput {
  locale: Locale;
  cache: MailCache;
  accountId: string;
  mailbox: Mailbox;
}

// The jobs reach the palette without a key.
const NO_KEYS: readonly Chord[] = [];

const COMMAND_IDS = new Map<Job["kind"], string>([
  ["markRead", "mailbox.markRead"],
  ["empty", "mailbox.empty"],
]);

// Starts a folder job; offline it runs nothing and the header says so. A
// worker that never takes the job ends it as failed with nothing done.
export function startJob(cache: MailCache, accountId: string, job: Job): void {
  if (!navigator.onLine) {
    askedOffline(accountId);
    return;
  }
  cache.job(accountId, job).catch((error: unknown) => {
    console.error(
      "mail: a folder job did not start",
      error instanceof Error ? error.message : String(error),
    );
    const failed: JobProgress = { job, done: 0, total: null, last: null, state: "failed" };
    heardJob(accountId, failed);
  });
}

// What a job's item does: marking read starts at once, emptying asks the
// confirm dialog first; offline neither runs and the header says so.
function runOf({ cache, accountId, mailbox }: FolderJobInput, kind: Job["kind"]): () => void {
  const job: Job = { kind, mailboxId: mailbox.id };
  if (kind === "markRead") {
    return () => {
      startJob(cache, accountId, job);
    };
  }
  return () => {
    if (navigator.onLine) {
      askEmpty({
        mailbox,
        confirm: () => {
          startJob(cache, accountId, job);
        },
      });
    } else {
      askedOffline(accountId);
    }
  };
}

function labelOf(kind: Job["kind"], mailbox: Mailbox, locale: Locale): string {
  return kind === "markRead"
    ? m.folder_mark_read({}, { locale })
    : m.folder_empty({ role: mailbox.role ?? "" }, { locale });
}

// The jobs a mailbox offers as items of its menu; none while a job of
// the account runs, since the worker runs one at a time.
export function jobItems(input: FolderJobInput, running: boolean): FolderItem[] {
  if (running) {
    return [];
  }
  return offeredJobs(input.mailbox).map((kind) => ({
    id: kind,
    label: labelOf(kind, input.mailbox, input.locale),
    danger: kind === "empty",
    run: runOf(input, kind),
  }));
}

function commandOf(items: readonly FolderItem[], kind: Job["kind"]): Command | null {
  const item = items.find((entry) => entry.id === kind);
  const id = COMMAND_IDS.get(kind);
  return item === undefined || id === undefined
    ? null
    : { id, label: item.label, group: "act", keys: NO_KEYS, run: item.run };
}

// Every job of the mailbox shown is in the palette too.
export function useFolderCommands(items: readonly FolderItem[]): void {
  useCommand(commandOf(items, "markRead"));
  useCommand(commandOf(items, "empty"));
}
