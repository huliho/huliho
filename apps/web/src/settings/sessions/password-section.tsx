// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { queryKeys } from "@huliho/state";
import { useQueryClient } from "@tanstack/react-query";
import { useRef } from "react";

import { useSignOut } from "../../auth/use-sign-out";
import { m } from "../../paraglide/messages.js";
import { getLocale } from "../../paraglide/runtime.js";
import { PasswordForm } from "../../password/password-form";
import { usePasswordChange } from "../../password/use-password-change";
import { SettingsSection } from "../settings-section";

export function PasswordSection() {
  const locale = getLocale();
  const queryClient = useQueryClient();
  const signOut = useSignOut(locale);
  const form = useRef<HTMLFormElement>(null);
  // The change ended every other session and moved this one onto a new row.
  const change = usePasswordChange(locale, signOut, async () => {
    form.current?.reset();
    await queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
  });
  return (
    <SettingsSection title={m.password_heading({}, { locale })}>
      <PasswordForm
        ref={form}
        locale={locale}
        mode="change"
        pending={change.pending}
        failure={change.failure}
        retryRemaining={change.retryRemaining}
        onSubmit={change.mutate}
      />
    </SettingsSection>
  );
}
