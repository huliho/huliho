// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { JobProgress, MailCache, Mailbox } from "@huliho/core";

import { toastManager } from "../../design-system/toast";
import type { Locale } from "../../paraglide/runtime.js";
import { startJob } from "./folder-jobs";
import { endSentence } from "./job-words";

interface JobEnd {
  cache: MailCache;
  accountId: string;
  progress: JobProgress;
  // The mailbox the job ran on, as this tab's tree names it.
  mailbox: Mailbox;
  locale: Locale;
}

// How a folder job ended, in a toast without undo: done and stopped say
// so plainly, a failure carries the alert and Try again, which asks the
// worker for the same job.
export function showJobEnd({ cache, accountId, progress, mailbox, locale }: JobEnd): void {
  const description = endSentence(progress, mailbox, locale);
  if (progress.state !== "failed") {
    toastManager.add({ description });
    return;
  }
  let toastId = "";
  const retry = (): void => {
    toastManager.close(toastId);
    startJob(cache, accountId, progress.job);
  };
  toastId = toastManager.add({
    description,
    data: { tone: "danger", retry },
    actionProps: { onClick: retry },
  });
}
