// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { CircleAlert } from "lucide-react";
import { useState } from "react";

import { cx } from "../design-system/cx";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import type { SignInFailure } from "./sign-in";
import styles from "./sign-in.module.css";

const SECONDS_PER_MINUTE = 60;

interface SignInFormProps {
  locale: Locale;
  pending: boolean;
  failure: SignInFailure;
  retryRemaining: number | null;
  onSubmit: (input: { login: string; password: string }) => void;
}

interface FieldProps {
  locale: Locale;
  held: boolean;
  wrongCredentials: boolean;
  value: string;
  onChange: (value: string) => void;
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

function buttonState(pending: boolean, held: boolean): string {
  if (pending) {
    return "pending";
  }
  return held ? "held" : "ready";
}

function HeldNotice({ locale, failure }: { locale: Locale; failure: SignInFailure }) {
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

function NameField({ locale, held, value, onChange }: Omit<FieldProps, "wrongCredentials">) {
  return (
    <label className={styles.field}>
      <span className={styles.label}>{m.signin_name_label({}, { locale })}</span>
      <input
        className={styles.input}
        type="text"
        autoComplete="username"
        autoCapitalize="none"
        spellCheck={false}
        required
        readOnly={held}
        value={value}
        onChange={(event) => {
          onChange(event.target.value);
        }}
      />
    </label>
  );
}

function PasswordField({ locale, held, wrongCredentials, value, onChange }: FieldProps) {
  return (
    <div className={styles.field}>
      <label className={styles.label} htmlFor="sign-in-password">
        {m.signin_password_label({}, { locale })}
      </label>
      <input
        id="sign-in-password"
        className={cx(styles.input, wrongCredentials ? styles.inputError : undefined)}
        type="password"
        autoComplete="current-password"
        required
        readOnly={held}
        aria-invalid={wrongCredentials}
        aria-describedby={wrongCredentials ? "sign-in-error" : undefined}
        value={value}
        onChange={(event) => {
          onChange(event.target.value);
        }}
      />
      {wrongCredentials && (
        <span className={styles.error} id="sign-in-error" role="alert">
          <CircleAlert className={styles.errorIcon} aria-hidden="true" />
          {m.signin_error_credentials({}, { locale })}
        </span>
      )}
    </div>
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
      <NameField locale={locale} held={held} value={login} onChange={setLogin} />
      <PasswordField
        locale={locale}
        held={held}
        wrongCredentials={wrongCredentials}
        value={password}
        onChange={setPassword}
      />
      <button
        className={styles.submit}
        type="submit"
        aria-disabled={held}
        data-state={buttonState(pending, held)}
      >
        {buttonLabel(locale, pending, retryRemaining)}
      </button>
    </form>
  );
}
