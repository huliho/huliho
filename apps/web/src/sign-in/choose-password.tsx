// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";

import { sessionQueryOptions } from "@huliho/state";
import { useSignOut } from "../auth/use-sign-out";
import { BrandMark } from "../design-system/brand-mark";
import { Button } from "../design-system/button";
import { m } from "../paraglide/messages.js";
import { getLocale } from "../paraglide/runtime.js";
import type { Locale } from "../paraglide/runtime.js";
import { PasswordForm } from "../password/password-form";
import type { PasswordFormProps } from "../password/password-form";
import { usePasswordChange } from "../password/use-password-change";
import styles from "./sign-in.module.css";

type FormState = Pick<PasswordFormProps, "pending" | "failure" | "retryRemaining" | "onSubmit">;

interface ChoosePasswordCardProps extends FormState {
  locale: Locale;
  onSignOut: () => void;
}

export function ChoosePasswordCard({ locale, onSignOut, ...form }: ChoosePasswordCardProps) {
  return (
    <>
      <BrandMark heading />
      <div className={styles.card}>
        <h2 className={styles.cardTitle}>{m.choose_password_heading({}, { locale })}</h2>
        <p className={styles.reason}>{m.choose_password_reason({}, { locale })}</p>
        <PasswordForm locale={locale} mode="forced" submitClassName={styles.submit} {...form} />
      </div>
      <Button className={styles.leave} onClick={onSignOut}>
        {m.signout_action({}, { locale })}
      </Button>
    </>
  );
}

// Where a one-time password lands: the session reaches nothing else
// until the user has chosen a password of their own.
export function ChoosePassword() {
  const locale = getLocale();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const signOut = useSignOut(locale);
  const change = usePasswordChange(locale, async () => {
    queryClient.removeQueries({ queryKey: sessionQueryOptions.queryKey });
    await navigate({ to: "/" });
  });
  return (
    <main className={styles.screen}>
      <div className={styles.column}>
        <ChoosePasswordCard
          locale={locale}
          pending={change.pending}
          failure={change.failure}
          retryRemaining={change.retryRemaining}
          onSubmit={change.mutate}
          onSignOut={signOut}
        />
      </div>
    </main>
  );
}
