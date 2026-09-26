// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { signOut } from "@huliho/core";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";

import { clearCache } from "../cache/client";
import { forgetRecent } from "../commands/recent";
import { toastManager } from "../design-system/toast";
import { forgetLastAccount } from "../mail/last-account";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";

// The local session ends either way, the cached mail, the remembered
// account and the commands last run with it; the server copy outlives
// only a failed revoke and ends at its timeout.
export function useSignOut(locale: Locale): () => void {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const mutation = useMutation({
    mutationFn: signOut,
    onSettled: async () => {
      await clearCache();
      queryClient.clear();
      forgetLastAccount();
      forgetRecent();
      await navigate({ to: "/sign-in" });
    },
    onError: () => {
      toastManager.add({ description: m.signout_failed({}, { locale }) });
    },
  });
  return () => {
    mutation.mutate();
  };
}
