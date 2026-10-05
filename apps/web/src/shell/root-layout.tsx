// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { sessionQueryOptions } from "@huliho/state";
import { useQueryClient } from "@tanstack/react-query";
import type { QueryClient } from "@tanstack/react-query";
import { Outlet } from "@tanstack/react-router";
import { useEffect } from "react";

import { useSignedOutElsewhere } from "../auth/use-signed-out-elsewhere";
import { installCacheListener } from "../cache/client";
import { installCommandListener } from "../commands/registry";
import { ToastProvider, Toasts, toastManager } from "../design-system/toast";
import { m } from "../paraglide/messages.js";
import { getLocale } from "../paraglide/runtime.js";
import { flushPendingOnPageHide, reapplyPendingAfterFetch } from "../undo/pending";

// Every tab hears of a change the server refused; the one in view that
// holds the session says so, never the link page or the sign-in screen.
function tellRefused(queryClient: QueryClient): void {
  const session = queryClient.getQueryData(sessionQueryOptions.queryKey);
  const signedIn = session !== undefined && session !== null;
  if (signedIn && document.visibilityState === "visible") {
    toastManager.add({ description: m.body_mark_read_failed({}, { locale: getLocale() }) });
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
      refused: () => {
        tellRefused(queryClient);
      },
    });
    return () => {
      uninstallCommands();
      uninstallFlush();
      uninstallReapply();
      uninstallCache();
    };
  }, [queryClient, signedOutElsewhere]);
  return (
    <ToastProvider>
      <Outlet />
      <Toasts />
    </ToastProvider>
  );
}
