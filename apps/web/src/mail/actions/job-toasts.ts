// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { JobProgress, MailCache, Mailbox } from "@huliho/core";

import { toastManager } from "../../design-system/toast";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { prefetchMenus } from "../menus-chunk";
import { startJob } from "./folder-jobs";

interface JobEnd {
  cache: MailCache;
  accountId: string;
  progress: JobProgress;
  // The mailbox the job ran on, as this tab's tree names it.
  mailbox: Mailbox;
  locale: Locale;
}

function showEnd({ cache, accountId, progress }: JobEnd, description: string): void {
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

// How a folder job ended, in a toast without undo: done and stopped say
// so plainly, a failure carries the alert and Try again, which asks the
// worker for the same job. The words ride the menus chunk; a chunk that
// does not come says so in a toast.
export async function showJobEnd(end: JobEnd): Promise<void> {
  try {
    const { endSentence } = await prefetchMenus();
    showEnd(end, endSentence(end.progress, end.mailbox, end.locale));
  } catch (error) {
    console.error(
      "mail: a job's words did not load",
      error instanceof Error ? error.message : String(error),
    );
    toastManager.add({
      description: m.pane_error({}, { locale: end.locale }),
      data: { tone: "danger" },
    });
  }
}
