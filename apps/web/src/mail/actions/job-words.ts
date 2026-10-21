// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { JobProgress, Mailbox } from "@huliho/core";

import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";

// The words of a job: emptying names the role's own word, marking read
// the mailbox's name.
function kindOf(progress: JobProgress, mailbox: Mailbox): string {
  if (progress.job.kind === "markRead") {
    return "markRead";
  }
  return mailbox.role === "junk" ? "junk" : "trash";
}

// A server that leaves out the total it was asked for has its count of
// what the job reached stand in for it.
function totalOf(progress: JobProgress): number {
  return Math.max(progress.total ?? 0, progress.done);
}

export function runningSentence(progress: JobProgress, mailbox: Mailbox, locale: Locale): string {
  return m.job_running({ kind: kindOf(progress, mailbox), mailbox: mailbox.name }, { locale });
}

// "500 of 1,204" once the first round has counted the job; null before.
export function countWords(progress: JobProgress, locale: Locale): string | null {
  return progress.total === null
    ? null
    : m.job_count({ done: progress.done, total: totalOf(progress) }, { locale });
}

// What a job that ended says, in the toast and the header's region.
export function endSentence(progress: JobProgress, mailbox: Mailbox, locale: Locale): string {
  const kind = kindOf(progress, mailbox);
  const { done } = progress;
  switch (progress.state) {
    case "canceled":
      return m.job_stopped({ done, total: totalOf(progress) }, { locale });
    case "failed":
      return m.job_failed(
        { kind, done, total: totalOf(progress), mailbox: mailbox.name },
        { locale },
      );
    default:
      return m.job_done({ kind, mailbox: mailbox.name }, { locale });
  }
}
