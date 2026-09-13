// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountsFailureCode, CredentialKind, Provider } from "@huliho/core";
import { useEffect, useRef } from "react";

import { retryLabel } from "../../auth/credential-notice";
import { Button } from "../../design-system/button";
import { Field } from "../../design-system/field";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { Callout } from "./callout";
import { credentialHint, credentialLabel, providerName, wrongCredential } from "./presets";
import styles from "./add-account.module.css";

interface FoundLine {
  host: string;
  onChange: () => void;
}

export interface CredentialStepProps {
  locale: Locale;
  address: string;
  provider: Provider;
  credentialKind: CredentialKind;
  // The found sentence with its Change; null on a reconnect.
  found: FoundLine | null;
  // What the connecting sentence names.
  host: string;
  secret: string;
  failure: AccountsFailureCode | null;
  pending: boolean;
  retryRemaining: number | null;
  onSecret: (value: string) => void;
  onSubmit: () => void;
}

function submitLabel(props: CredentialStepProps): string {
  const { locale } = props;
  const countdown = retryLabel(locale, props.retryRemaining);
  if (countdown !== null) {
    return countdown;
  }
  if (props.pending) {
    return m.account_connecting_button({}, { locale });
  }
  return props.found === null
    ? m.account_connect({}, { locale })
    : m.account_continue({}, { locale });
}

function FoundCallout({
  locale,
  address,
  provider,
  found,
  held,
}: CredentialStepProps & { held: boolean }) {
  if (found === null) {
    return null;
  }
  const inputs = { provider: providerName(provider, address), host: found.host };
  return (
    <Callout tone="info">
      {m.account_found(inputs, { locale })}{" "}
      <Button variant="plain" type="button" held={held} onClick={found.onChange}>
        {m.account_change({}, { locale })}
      </Button>
    </Callout>
  );
}

function SecretField(props: CredentialStepProps & { held: boolean }) {
  const { locale, credentialKind: kind } = props;
  const input = useRef<HTMLInputElement>(null);
  const refused = props.failure === "upstream_credentials";
  useEffect(() => {
    input.current?.focus();
  }, []);
  // A refusal lands while the step stays mounted, so the sentence gets the cursor too.
  useEffect(() => {
    if (refused) {
      input.current?.focus();
    }
  }, [refused]);
  return (
    <Field
      ref={input}
      label={credentialLabel(kind, locale)}
      type="password"
      name="secret"
      autoComplete={kind === "password" ? "current-password" : "off"}
      required
      readOnly={props.held}
      value={props.secret}
      onChange={(event) => {
        props.onSecret(event.target.value);
      }}
      error={props.failure === "upstream_credentials" ? wrongCredential(kind, locale) : undefined}
    />
  );
}

// The credential field for the server found or the row to reconnect; a
// provider without a password route gets its admin sentence instead.
export function CredentialStep(props: CredentialStepProps) {
  const { locale, credentialKind: kind } = props;
  const held = props.pending || props.retryRemaining !== null;
  const hint = credentialHint(props.provider, locale);
  const askable = kind !== "oauth";
  return (
    <form
      className={styles.form}
      onSubmit={(event) => {
        event.preventDefault();
        if (!held) {
          props.onSubmit();
        }
      }}
    >
      <Field
        label={m.account_address_label({}, { locale })}
        type="email"
        readOnly
        value={props.address}
      />
      <FoundCallout {...props} held={held} />
      {hint !== null && <Callout tone="warn">{hint}</Callout>}
      {askable && <SecretField {...props} held={held} />}
      {props.pending && (
        <output className={styles.status}>
          {m.account_connecting({ host: props.host }, { locale })}
        </output>
      )}
      {askable && (
        <Button
          variant="primary"
          className={styles.submit}
          type="submit"
          held={held}
          pending={props.pending}
        >
          {submitLabel(props)}
        </Button>
      )}
    </form>
  );
}
