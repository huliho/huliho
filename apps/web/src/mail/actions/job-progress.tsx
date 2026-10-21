// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { JobProgress, MailCache, Mailbox } from "@huliho/core";
import { WifiOff } from "lucide-react";

import { Button } from "../../design-system/button";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { countWords, runningSentence } from "./job-words";
import styles from "./job-progress.module.css";

const PERCENT = 100;

interface JobProgressProps {
  locale: Locale;
  cache: MailCache;
  accountId: string;
  progress: JobProgress;
  // The mailbox the job runs on, which its sentence names.
  mailbox: Mailbox;
}

function shareOf({ done, total }: JobProgress): number {
  return total === null || total === 0
    ? 0
    : Math.min(PERCENT, Math.round((done / total) * PERCENT));
}

// While a folder job runs it takes the header: the sentence, the count
// in mono, Cancel at the end where the bulk bar keeps Close and a
// hairline on the foot edge at done over total. Cancel ends the job
// after the round that runs.
export function JobProgressBar({ locale, cache, accountId, progress, mailbox }: JobProgressProps) {
  const count = countWords(progress, locale);
  return (
    <div className={styles.progress} data-progress>
      <p className={styles.words}>
        <span>{runningSentence(progress, mailbox, locale)}</span>
        {count !== null && <span className={styles.count}>{count}</span>}
      </p>
      <Button
        className={styles.cancel}
        onClick={() => {
          cache.cancelJob(accountId).catch((error: unknown) => {
            console.error(
              "mail: canceling a folder job failed",
              error instanceof Error ? error.message : String(error),
            );
          });
        }}
      >
        {m.cancel_action({}, { locale })}
      </Button>
      <span
        className={styles.hairline}
        aria-hidden="true"
        style={{ inlineSize: `${String(shareOf(progress))}%` }}
      />
    </div>
  );
}

// A job asked offline ran nothing; the header says so in the same place.
export function OfflineAsk({ locale }: { locale: Locale }) {
  return (
    <p className={styles.offline}>
      <WifiOff className={styles.icon} aria-hidden="true" />
      {m.job_offline({}, { locale })}
    </p>
  );
}
