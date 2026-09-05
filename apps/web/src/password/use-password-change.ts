// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { changePassword } from "@huliho/core";
import type { PasswordChangeInput } from "@huliho/core";

import { useCredentialMutation } from "../auth/use-credential-mutation";
import type { CredentialMutation } from "../auth/use-credential-mutation";
import { toastManager } from "../design-system/toast";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";

// One change for the section and the forced step: both say the same
// thing on success. A session gone meanwhile ends here for both.
export function usePasswordChange(
  locale: Locale,
  signOut: () => void,
  onSaved: () => Promise<void> | void,
): CredentialMutation<PasswordChangeInput> {
  return useCredentialMutation(changePassword, {
    onSuccess: async () => {
      toastManager.add({ description: m.password_changed_toast({}, { locale }) });
      await onSaved();
    },
    onFailure: (failure) => {
      if (failure === "unauthenticated") {
        toastManager.add({ description: m.session_ended({}, { locale }) });
        signOut();
      }
    },
  });
}
