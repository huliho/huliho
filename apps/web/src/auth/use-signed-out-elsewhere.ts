// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { queryKeys } from "@huliho/state";
import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useRef } from "react";

import { clearCache } from "../cache/client";
import { toastManager } from "../design-system/toast";
import { m } from "../paraglide/messages.js";
import { getLocale } from "../paraglide/runtime.js";

// A sign-out in another tab ends this one too: this tab's worker stops
// and the database goes, this tab drops what it holds, says so and shows
// the sign-in screen. A tab without a session has nothing to end, and a
// tab that is ending already ends once.
export function useSignedOutElsewhere(): () => void {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const ending = useRef(false);
  const end = async (): Promise<void> => {
    ending.current = true;
    try {
      toastManager.add({ description: m.session_ended({}, { locale: getLocale() }) });
      await clearCache();
      queryClient.clear();
      await navigate({ to: "/sign-in" });
    } finally {
      ending.current = false;
    }
  };
  return () => {
    const session = queryClient.getQueryData(queryKeys.session);
    if (!ending.current && session !== undefined && session !== null) {
      void end();
    }
  };
}
