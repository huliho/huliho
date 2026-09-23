// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { PreferencesError, setPreference, withPreference } from "@huliho/core";
import type { PreferenceChange, Preferences } from "@huliho/core";
import { queryKeys } from "@huliho/state";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { useSessionEnded } from "../../auth/use-session-ended";
import { toastManager } from "../../design-system/toast";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";

// A choice lands in the cache first, so the screen follows at once; the
// server hears it next. A refusal puts the server's word back and says so.
export function usePreference(locale: Locale): (change: PreferenceChange) => void {
  const queryClient = useQueryClient();
  const sessionEnded = useSessionEnded(locale);
  const mutation = useMutation({
    mutationFn: setPreference,
    onMutate: async (change) => {
      // A fetch already in flight would land its older word on top of the choice.
      await queryClient.cancelQueries({ queryKey: queryKeys.preferences });
      queryClient.setQueryData<Preferences>(queryKeys.preferences, (current) =>
        withPreference(current ?? {}, change),
      );
    },
    onError: (error) => {
      if (error instanceof PreferencesError && error.code === "unauthenticated") {
        sessionEnded();
        return;
      }
      void queryClient.invalidateQueries({ queryKey: queryKeys.preferences });
      toastManager.add({ description: m.appearance_save_failed({}, { locale }) });
    },
  });
  return (change) => {
    mutation.mutate(change);
  };
}
