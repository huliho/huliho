// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useState } from "react";

import type { SignInFailureCode } from "@huliho/core";
import { Button } from "../design-system/button";
import { Field } from "../design-system/field";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import styles from "./sign-in.module.css";

const SECONDS_PER_MINUTE = 60;

interface SignInFormProps {
  locale: Locale;
  pending: boolean;
  failure: SignInFailureCode | null;
  retryRemaining: number | null;
  onSubmit: (input: { login: string; password: string }) => void;
}

function countdownLabel(locale: Locale, seconds: number): string {
  return new Intl.DurationFormat(locale, { style: "digital", hoursDisplay: "auto" }).format({
    minutes: Math.floor(seconds / SECONDS_PER_MINUTE),
    seconds: seconds % SECONDS_PER_MINUTE,
  });
}

function buttonLabel(locale: Locale, pending: boolean, retryRemaining: number | null): string {
  if (retryRemaining !== null) {
    return m.signin_retry_countdown({ time: countdownLabel(locale, retryRemaining) }, { locale });
  }
  return pending ? m.signin_submitting({}, { locale }) : m.signin_submit({}, { locale });
}

function HeldNotice({ locale, failure }: { locale: Locale; failure: SignInFailureCode | null }) {
  if (failure !== "rate_limited" && failure !== "unavailable") {
    return null;
  }
  const message =
    failure === "rate_limited"
      ? m.signin_error_rate_limited({}, { locale })
      : m.signin_error_unavailable({}, { locale });
  return (
    <p className={styles.held} role="alert">
      {message}
    </p>
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
      <HeldNotice locale={locale} failure={failure} />
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
        {buttonLabel(locale, pending, retryRemaining)}
      </Button>
    </form>
  );
}
