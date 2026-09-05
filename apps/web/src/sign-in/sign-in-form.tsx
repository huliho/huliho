// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useState } from "react";

import type { CredentialFailureCode } from "@huliho/core";
import { CredentialNotice, retryLabel } from "../auth/credential-notice";
import { Button } from "../design-system/button";
import { Field } from "../design-system/field";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import styles from "./sign-in.module.css";

interface SignInFormProps {
  locale: Locale;
  pending: boolean;
  failure: CredentialFailureCode | null;
  retryRemaining: number | null;
  onSubmit: (input: { login: string; password: string }) => void;
}

function submitLabel(locale: Locale, pending: boolean, retryRemaining: number | null): string {
  return (
    retryLabel(locale, retryRemaining) ??
    (pending ? m.signin_submitting({}, { locale }) : m.signin_submit({}, { locale }))
  );
}

export function SignInForm({
  locale,
  pending,
  failure,
  retryRemaining,
  onSubmit,
}: SignInFormProps) {
  const [login, setLogin] = useState("");
  const [password, setPassword] = useState("");
  const held = pending || failure === "rate_limited";
  const wrongCredentials = failure === "invalid_credentials";

  return (
    <form
      className={styles.card}
      onSubmit={(event) => {
        event.preventDefault();
        if (!held) {
          onSubmit({ login, password });
        }
      }}
    >
      <CredentialNotice locale={locale} failure={failure} />
      <Field
        label={m.signin_name_label({}, { locale })}
        type="text"
        autoComplete="username"
        autoCapitalize="none"
        spellCheck={false}
        required
        readOnly={held}
        value={login}
        onChange={(event) => {
          setLogin(event.target.value);
        }}
      />
      <Field
        label={m.signin_password_label({}, { locale })}
        type="password"
        autoComplete="current-password"
        required
        readOnly={held}
        value={password}
        onChange={(event) => {
          setPassword(event.target.value);
        }}
        error={wrongCredentials ? m.signin_error_credentials({}, { locale }) : undefined}
      />
      <Button
        variant="primary"
        className={styles.submit}
        type="submit"
        held={held}
        pending={pending}
      >
        {submitLabel(locale, pending, retryRemaining)}
      </Button>
    </form>
  );
}
