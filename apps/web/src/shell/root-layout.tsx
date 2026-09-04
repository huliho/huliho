// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useQueryClient } from "@tanstack/react-query";
import { Outlet } from "@tanstack/react-router";
import { useEffect } from "react";

import { installCommandListener } from "../commands/registry";
import { ToastProvider, Toasts } from "../design-system/toast";
import { flushPendingOnPageHide, reapplyPendingAfterFetch } from "../undo/pending";

export function RootLayout() {
  const queryClient = useQueryClient();
  useEffect(() => {
    const uninstallCommands = installCommandListener();
    const uninstallFlush = flushPendingOnPageHide();
    const uninstallReapply = reapplyPendingAfterFetch(queryClient);
    return () => {
      uninstallCommands();
      uninstallFlush();
      uninstallReapply();
    };
  }, [queryClient]);
  return (
    <ToastProvider>
      <Outlet />
      <Toasts />
    </ToastProvider>
  );
}
