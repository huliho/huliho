// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { CredentialFailureCode } from "@huliho/core";

import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import styles from "./credential-notice.module.css";

const SECONDS_PER_MINUTE = 60;

interface CredentialNoticeProps {
  locale: Locale;
  failure: CredentialFailureCode | null;
}

// The refusals a form cannot pin on one field: the limiter and an unreachable server.
export function CredentialNotice({ locale, failure }: CredentialNoticeProps) {
  if (failure !== "rate_limited" && failure !== "unavailable") {
    return null;
  }
  const message =
    failure === "rate_limited"
      ? m.signin_error_rate_limited({}, { locale })
      : m.signin_error_unavailable({}, { locale });
  return (
    <p className={styles.notice} role="alert">
      {message}
    </p>
  );
}

// What a held submit says while the limiter counts down; null once it may submit.
export function retryLabel(locale: Locale, retryRemaining: number | null): string | null {
  if (retryRemaining === null) {
    return null;
  }
  const time = new Intl.DurationFormat(locale, { style: "digital", hoursDisplay: "auto" }).format({
    minutes: Math.floor(retryRemaining / SECONDS_PER_MINUTE),
    seconds: retryRemaining % SECONDS_PER_MINUTE,
  });
  return m.signin_retry_countdown({ time }, { locale });
}
