// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountsFailureCode, TlsMode } from "@huliho/core";
import { useEffect, useRef, useState } from "react";
import type { RefObject, SubmitEvent } from "react";

import { retryLabel } from "../../auth/credential-notice";
import { Button } from "../../design-system/button";
import { Field } from "../../design-system/field";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { incomingPort, isSessionUrl, manualTarget, outgoingOf, usernameOf } from "./manual-target";
import type { BuiltTarget, ManualValues, OutgoingValues } from "./manual-target";
import { PORT_MAX, PORT_MIN, wrongCredential } from "./presets";
import styles from "./add-account.module.css";

interface ManualStepProps {
  locale: Locale;
  // The address typed on the first screen, the username until edited.
  address: string;
  values: ManualValues;
  failure: AccountsFailureCode | null;
  pending: boolean;
  retryRemaining: number | null;
  onChange: (patch: Partial<ManualValues>) => void;
  onConnect: (built: BuiltTarget) => void;
}

interface SectionProps extends ManualStepProps {
  held: boolean;
}

interface IncomingProps extends SectionProps {
  server: RefObject<HTMLInputElement | null>;
  serverIssue: boolean;
  onServer: (value: string) => void;
}

interface PortAndTlsProps {
  locale: Locale;
  held: boolean;
  port: string;
  tls: TlsMode;
  onPort: (value: string) => void;
  onTls: (value: TlsMode) => void;
}

function tlsOf(value: string): TlsMode {
  return value === "starttls" ? "starttls" : "implicit";
}

function submitLabel(locale: Locale, pending: boolean, retryRemaining: number | null): string {
  const countdown = retryLabel(locale, retryRemaining);
  if (countdown !== null) {
    return countdown;
  }
  return pending ? m.account_connecting_button({}, { locale }) : m.account_connect({}, { locale });
}

function PortAndTls({ locale, held, port, tls, onPort, onTls }: PortAndTlsProps) {
  return (
    <div className={styles.row}>
      <Field
        label={m.account_port_label({}, { locale })}
        type="number"
        inputMode="numeric"
        min={PORT_MIN}
        max={PORT_MAX}
        required
        readOnly={held}
        value={port}
        onChange={(event) => {
          onPort(event.target.value);
        }}
      />
      <Field
        label={m.account_encryption_label({}, { locale })}
        value={tls}
        disabled={held}
        onChange={(event) => {
          onTls(tlsOf(event.target.value));
        }}
      >
        <option value="implicit">{m.account_tls_option({}, { locale })}</option>
        <option value="starttls">{m.account_starttls_option({}, { locale })}</option>
      </Field>
    </div>
  );
}

function Incoming(props: IncomingProps) {
  const { locale, values, held, onChange, server, serverIssue, onServer } = props;
  const jmap = isSessionUrl(values.server);
  return (
    <>
      <Field
        ref={server}
        label={m.account_server_label({}, { locale })}
        type="text"
        name="server"
        autoComplete="off"
        autoCapitalize="none"
        spellCheck={false}
        required
        readOnly={held}
        value={values.server}
        onChange={(event) => {
          onServer(event.target.value);
        }}
        error={serverIssue ? m.account_error_server({}, { locale }) : undefined}
      />
      {!jmap && (
        <PortAndTls
          locale={locale}
          held={held}
          port={incomingPort(values)}
          tls={values.tls}
          onPort={(port) => {
            onChange({ port });
          }}
          onTls={(tls) => {
            onChange({ tls });
          }}
        />
      )}
      {!jmap && <Username {...props} />}
    </>
  );
}

function Username({ locale, address, values, held, onChange }: SectionProps) {
  return (
    <Field
      label={m.account_username_label({}, { locale })}
      type="text"
      autoComplete="username"
      autoCapitalize="none"
      spellCheck={false}
      required
      readOnly={held}
      value={usernameOf(values, address)}
      onChange={(event) => {
        onChange({ username: event.target.value });
      }}
    />
  );
}

function Password({ locale, values, failure, held, onChange }: SectionProps) {
  const input = useRef<HTMLInputElement>(null);
  const refused = failure === "upstream_credentials";
  // A refusal lands while the form stays mounted, so the sentence gets the cursor too.
  useEffect(() => {
    if (refused) {
      input.current?.focus();
    }
  }, [refused]);
  return (
    <Field
      ref={input}
      label={m.account_password_label({}, { locale })}
      type="password"
      autoComplete="current-password"
      required
      readOnly={held}
      value={values.password}
      onChange={(event) => {
        onChange({ password: event.target.value });
      }}
      error={failure === "upstream_credentials" ? wrongCredential("password", locale) : undefined}
    />
  );
}

function Outgoing({ locale, values, held, onChange }: SectionProps) {
  const outgoing = outgoingOf(values);
  const patch = (part: Partial<OutgoingValues>): void => {
    onChange({ outgoing: { ...values.outgoing, ...part } });
  };
  return (
    <details className={styles.disclosure}>
      <summary>{m.account_outgoing_server({}, { locale })}</summary>
      <Field
        label={m.account_server_label({}, { locale })}
        type="text"
        autoComplete="off"
        autoCapitalize="none"
        spellCheck={false}
        required
        readOnly={held}
        value={outgoing.host}
        onChange={(event) => {
          patch({ host: event.target.value });
        }}
      />
      <PortAndTls
        locale={locale}
        held={held}
        port={outgoing.port}
        tls={outgoing.tls}
        onPort={(port) => {
          patch({ port });
        }}
        onTls={(tls) => {
          patch({ tls });
        }}
      />
    </details>
  );
}

// The last resort: every field typed by hand. A Server value that is a
// session URL turns the form into a JMAP one.
export function ManualStep(props: ManualStepProps) {
  const { locale, address, values, pending, retryRemaining, onChange } = props;
  const [serverIssue, setServerIssue] = useState(false);
  const server = useRef<HTMLInputElement>(null);
  useEffect(() => {
    server.current?.focus();
  }, []);
  const held = pending || retryRemaining !== null;
  const jmap = isSessionUrl(values.server);
  const submit = (event: SubmitEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (held) {
      return;
    }
    const built = manualTarget(values, address);
    if (built === null) {
      setServerIssue(true);
      server.current?.focus();
      return;
    }
    props.onConnect(built);
  };
  return (
    <form className={styles.form} onSubmit={submit}>
      <Incoming
        {...props}
        held={held}
        server={server}
        serverIssue={serverIssue}
        onServer={(value) => {
          setServerIssue(false);
          onChange({ server: value });
        }}
      />
      <Password {...props} held={held} />
      {!jmap && <Outgoing {...props} held={held} />}
      {pending && (
        <output className={styles.status}>
          {m.account_connecting({ host: values.server.trim() }, { locale })}
        </output>
      )}
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
