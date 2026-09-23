// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useQueryClient } from "@tanstack/react-query";
import { Outlet } from "@tanstack/react-router";
import { useEffect } from "react";

import { useSignedOutElsewhere } from "../auth/use-signed-out-elsewhere";
import { installCacheListener } from "../cache/client";
import { installCommandListener } from "../commands/registry";
import { ToastProvider, Toasts } from "../design-system/toast";
import { flushPendingOnPageHide, reapplyPendingAfterFetch } from "../undo/pending";

export function RootLayout() {
  const queryClient = useQueryClient();
  const signedOutElsewhere = useSignedOutElsewhere();
  useEffect(() => {
    const uninstallCommands = installCommandListener();
    const uninstallFlush = flushPendingOnPageHide();
    const uninstallReapply = reapplyPendingAfterFetch(queryClient);
    const uninstallCache = installCacheListener(queryClient, signedOutElsewhere);
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
