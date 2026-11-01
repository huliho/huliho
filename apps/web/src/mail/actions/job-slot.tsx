// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { JobProgress, MailCache, Mailbox } from "@huliho/core";
import { Suspense, use, useEffect, useState } from "react";
import type { RefObject } from "react";

import { Skeleton } from "../../design-system/skeleton";
import type { Locale } from "../../paraglide/runtime.js";
import { prefetchMenus } from "../menus-chunk";
import { useStatusText } from "../status-text";
import { onJobEnd } from "./job-store";

// A job of the account and the mailbox it runs on, as this tree has it.
export interface ShownJob {
  progress: JobProgress;
  box: Mailbox;
}

// What the header's status region says: the running job, the answer to
// a job asked offline or how the account's last job ended; null for
// nothing.
export type JobSaid =
  | { kind: "running"; job: ShownJob }
  | { kind: "offline" }
  | { kind: "ended"; job: ShownJob }
  | null;

interface SlotProps {
  locale: Locale;
  cache: MailCache;
  accountId: string;
  // The running job; null for the answer to a job asked offline.
  job: ShownJob | null;
  // The class of the still shape that holds the place meanwhile.
  still: string | undefined;
}

function LoadedSlot({ locale, cache, accountId, job }: Omit<SlotProps, "still">) {
  const { JobProgressBar, OfflineAsk } = use(prefetchMenus());
  return job === null ? (
    <OfflineAsk locale={locale} />
  ) : (
    <JobProgressBar
      locale={locale}
      cache={cache}
      accountId={accountId}
      progress={job.progress}
      mailbox={job.box}
    />
  );
}

// The running job's bar, else the answer to a job asked offline. Both
// ride the menus chunk, so a still shape holds the place until it is in.
export function JobSlot({ still, ...props }: SlotProps) {
  return (
    <Suspense fallback={<Skeleton className={still} />}>
      <LoadedSlot {...props} />
    </Suspense>
  );
}

// How the account's last job ended, as this tree names its mailbox.
export function useJobEnded(accountId: string, mailboxes: readonly Mailbox[]): ShownJob | null {
  const [ended, setEnded] = useState<ShownJob | null>(null);
  useEffect(
    () =>
      onJobEnd((endedOn, progress) => {
        const box = mailboxes.find((row) => row.id === progress.job.mailboxId);
        if (endedOn === accountId && box !== undefined) {
          setEnded({ progress, box });
        }
      }),
    [accountId, mailboxes],
  );
  return ended;
}

interface WordsProps {
  locale: Locale;
  said: JobSaid;
  regionRef: RefObject<HTMLElement | null>;
}

function LoadedWords({ locale, said, regionRef }: WordsProps) {
  const { endSentence, offlineSentence, runningSentence } = use(prefetchMenus());
  let text = "";
  if (said?.kind === "running") {
    text = runningSentence(said.job.progress, said.job.box, locale);
  } else if (said?.kind === "ended") {
    text = endSentence(said.job.progress, said.job.box, locale);
  } else if (said?.kind === "offline") {
    text = offlineSentence(locale);
  }
  useStatusText(regionRef, text);
  return null;
}

// Writes what the header says into its status region; the words ride the
// menus chunk with the bar.
export function JobWords(props: WordsProps) {
  return (
    <Suspense fallback={null}>
      <LoadedWords {...props} />
    </Suspense>
  );
}
