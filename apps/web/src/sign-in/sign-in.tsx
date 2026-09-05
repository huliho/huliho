// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";

import { signIn } from "@huliho/core";
import { sessionQueryOptions } from "@huliho/state";
import { useCredentialMutation } from "../auth/use-credential-mutation";
import { BrandMark } from "../design-system/brand-mark";
import { LegalNotices } from "../legal/legal-notices";
import { m } from "../paraglide/messages.js";
import { getLocale } from "../paraglide/runtime.js";
import { SignInForm } from "./sign-in-form";
import styles from "./sign-in.module.css";

export function SignIn() {
  const locale = getLocale();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const mutation = useCredentialMutation(
    (input: { login: string; password: string }) => signIn(input.login, input.password),
    {
      // The guard on "/" reads the fresh session and sends a forced one on to its step.
      onSuccess: async () => {
        queryClient.removeQueries({ queryKey: sessionQueryOptions.queryKey });
        await navigate({ to: "/" });
      },
    },
  );

  return (
    <main className={styles.screen}>
      <div className={styles.column}>
        <BrandMark heading />
        <SignInForm
          locale={locale}
          pending={mutation.pending}
          failure={mutation.failure}
          retryRemaining={mutation.retryRemaining}
          onSubmit={mutation.mutate}
        />
        <p className={styles.adminNote}>{m.signin_admin_note({}, { locale })}</p>
        <LegalNotices locale={locale} />
      </div>
    </main>
  );
}
