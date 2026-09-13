// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { FoundServer } from "@huliho/core";
import { useEffect, useRef } from "react";

import { retryLabel } from "../../auth/credential-notice";
import { Button } from "../../design-system/button";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { providerName } from "./presets";
import styles from "./add-account.module.css";

interface ConfirmHostStepProps {
  locale: Locale;
  address: string;
  found: FoundServer;
  pending: boolean;
  retryRemaining: number | null;
  onConnect: () => void;
  onDifferentServer: () => void;
}

function connectLabel(locale: Locale, pending: boolean, retryRemaining: number | null): string {
  const countdown = retryLabel(locale, retryRemaining);
  if (countdown !== null) {
    return countdown;
  }
  return pending ? m.account_connecting_button({}, { locale }) : m.account_connect({}, { locale });
}

// The host is confirmed before the credential travels, so the button
// takes focus: Enter is the confirmation.
export function ConfirmHostStep({
  locale,
  address,
  found,
  pending,
  retryRemaining,
  onConnect,
  onDifferentServer,
}: ConfirmHostStepProps) {
  const connect = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    connect.current?.focus();
  }, []);
  const held = pending || retryRemaining !== null;
  const question =
    found.credentialKind === "apiToken"
      ? m.account_confirm_token({}, { locale })
      : m.account_confirm_password({}, { locale });
  return (
    <div className={styles.form}>
      <p className={styles.lead}>{question}</p>
      <div className={styles.hostBox}>
        <span className={styles.host}>{found.host}</span>
        <span className={styles.hostMeta}>
          {m.account_encrypted({ provider: providerName(found.provider, address) }, { locale })}
        </span>
      </div>
      {pending && (
        <output className={styles.status}>
          {m.account_connecting({ host: found.host }, { locale })}
        </output>
      )}
      <Button
        ref={connect}
        variant="primary"
        className={styles.submit}
        held={held}
        pending={pending}
        onClick={onConnect}
      >
        {connectLabel(locale, pending, retryRemaining)}
      </Button>
      <Button variant="plain" className={styles.centered} held={held} onClick={onDifferentServer}>
        {m.account_different_server({}, { locale })}
      </Button>
    </div>
  );
}
