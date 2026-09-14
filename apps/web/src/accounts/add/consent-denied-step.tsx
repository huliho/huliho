// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { SignInProvider } from "@huliho/core";
import { useEffect, useRef } from "react";

import { Button } from "../../design-system/button";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { Callout } from "./callout";
import type { ConsentRefusal } from "./flow";
import { passwordRoute, signInName } from "./presets";
import styles from "./add-account.module.css";

interface ConsentDeniedStepProps {
  locale: Locale;
  signIn: SignInProvider;
  address: string;
  cause: ConsentRefusal;
  // A reconnect keeps the row's kind, so it never offers the password route.
  passwordOffered: boolean;
  onRetry: () => void;
  onUsePassword: () => void;
}

interface Inputs {
  provider: string;
  address: string;
}

// One sentence per way a consent ends without an account.
function refusalText(
  cause: ConsentRefusal,
  inputs: Inputs,
  passwordOffered: boolean,
  locale: Locale,
): string {
  switch (cause) {
    case "accessDenied":
      return passwordOffered
        ? m.account_consent_denied(inputs, { locale })
        : m.account_consent_denied_retry(inputs, { locale });
    case "upstreamCredentials":
      return m.account_consent_wrong_account(inputs, { locale });
    case "smtpAuthUnavailable":
      return m.account_error_smtp_auth({}, { locale });
    case "gone":
      return m.account_consent_expired(inputs, { locale });
    default:
      return m.account_consent_failed(inputs, { locale });
  }
}

export function ConsentDeniedStep(props: ConsentDeniedStepProps) {
  const { locale, signIn, cause, onRetry, onUsePassword } = props;
  const retry = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    retry.current?.focus();
  }, []);
  const offered = props.passwordOffered && passwordRoute(signIn);
  const inputs = { provider: signInName(signIn), address: props.address };
  return (
    <div className={styles.form}>
      <Callout tone="danger" live="alert">
        {refusalText(cause, inputs, offered, locale)}
      </Callout>
      <Button ref={retry} variant="primary" className={styles.submit} onClick={onRetry}>
        {m.retry_action({}, { locale })}
      </Button>
      {offered && (
        <Button variant="plain" className={styles.centered} onClick={onUsePassword}>
          {m.account_use_password({}, { locale })}
        </Button>
      )}
    </div>
  );
}
