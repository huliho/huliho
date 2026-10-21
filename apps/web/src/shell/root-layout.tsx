// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { JobProgress } from "@huliho/core";
import { mailboxesQueryOptions, sessionQueryOptions } from "@huliho/state";
import { useQueryClient } from "@tanstack/react-query";
import type { QueryClient } from "@tanstack/react-query";
import { Outlet } from "@tanstack/react-router";
import { useEffect } from "react";

import { useSignedOutElsewhere } from "../auth/use-signed-out-elsewhere";
import { installCacheListener, mailCache } from "../cache/client";
import type { RefusedMessage } from "../cache/client";
import { installCommandListener } from "../commands/registry";
import { ToastProvider, Toasts } from "../design-system/toast";
import { heardJob, onJobEnd } from "../mail/actions/job-store";
import { showJobEnd } from "../mail/actions/job-toasts";
import { showRefused } from "../mail/actions/refusals";
import { getLocale } from "../paraglide/runtime.js";
import { flushPendingOnPageHide, reapplyPendingAfterFetch } from "../undo/pending";

// Whether this tab is the one that speaks: in view and holding the
// session, never the link page or the sign-in screen.
function tells(queryClient: QueryClient): boolean {
  const session = queryClient.getQueryData(sessionQueryOptions.queryKey);
  return session !== undefined && session !== null && document.visibilityState === "visible";
}

// Every tab hears of a change the server refused; the one that speaks
// says so.
function heardRefusal(queryClient: QueryClient, message: RefusedMessage): void {
  showRefused(message, getLocale(), tells(queryClient));
}

// Every tab hears a folder job end; the one that speaks says how, where
// its tree names the mailbox the job ran on.
function heardJobEnd(queryClient: QueryClient, accountId: string, progress: JobProgress): void {
  const mailboxes = queryClient.getQueryData(mailboxesQueryOptions(mailCache, accountId).queryKey);
  const mailbox = mailboxes?.find((row) => row.id === progress.job.mailboxId);
  if (mailbox !== undefined && tells(queryClient)) {
    showJobEnd({ cache: mailCache, accountId, progress, mailbox, locale: getLocale() });
  }
}

export function RootLayout() {
  const queryClient = useQueryClient();
  const signedOutElsewhere = useSignedOutElsewhere();
  useEffect(() => {
    const uninstallCommands = installCommandListener();
    const uninstallFlush = flushPendingOnPageHide();
    const uninstallReapply = reapplyPendingAfterFetch(queryClient);
    const uninstallCache = installCacheListener(queryClient, {
      cleared: signedOutElsewhere,
      refused: (message) => {
        heardRefusal(queryClient, message);
      },
      job: (message) => {
        heardJob(message.accountId, message.progress);
      },
    });
    const uninstallJobEnds = onJobEnd((accountId, progress) => {
      heardJobEnd(queryClient, accountId, progress);
    });
    return () => {
      uninstallCommands();
      uninstallFlush();
      uninstallReapply();
      uninstallCache();
      uninstallJobEnds();
    };
  }, [queryClient, signedOutElsewhere]);
  return (
    <ToastProvider>
      <Outlet />
      <Toasts />
    </ToastProvider>
  );
}
