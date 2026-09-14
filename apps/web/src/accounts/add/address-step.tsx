// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountsFailureCode, SignInProvider } from "@huliho/core";
import { useEffect, useRef } from "react";

import { retryLabel } from "../../auth/credential-notice";
import { Button } from "../../design-system/button";
import { Field } from "../../design-system/field";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { Callout } from "./callout";
import { signInName } from "./presets";
import styles from "./add-account.module.css";

export type AddressStepName = "typing" | "detecting" | "notFound";

interface AddressHandlers {
  address: (value: string) => void;
  password: (value: string) => void;
  continue: () => void;
  signIn: (signIn: SignInProvider) => void;
  enterDetails: () => void;
}

interface AddressStepProps {
  locale: Locale;
  step: AddressStepName;
  address: string;
  password: string;
  failure: AccountsFailureCode | null;
  retryRemaining: number | null;
  // The sign-in providers the instance can start a consent with.
  signInProviders: SignInProvider[];
  on: AddressHandlers;
}

interface AddressFieldsProps extends AddressStepProps {
  held: boolean;
}

function submitLabel(locale: Locale, step: AddressStepName, retryRemaining: number | null): string {
  const countdown = retryLabel(locale, retryRemaining);
  if (countdown !== null) {
    return countdown;
  }
  return step === "notFound" ? m.retry_action({}, { locale }) : m.account_continue({}, { locale });
}

function AddressFields({ locale, step, address, password, failure, held, on }: AddressFieldsProps) {
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    input.current?.focus();
  }, []);
  return (
    <>
      <Field
        ref={input}
        label={m.account_address_label({}, { locale })}
        type="email"
        name="address"
        autoComplete="email"
        inputMode="email"
        autoCapitalize="none"
        spellCheck={false}
        required
        readOnly={held}
        value={address}
        onChange={(event) => {
          on.address(event.target.value);
        }}
        error={failure === "invalid_request" ? m.account_error_address({}, { locale }) : undefined}
      />
      {step !== "notFound" && (
        <Field
          label={m.account_password_label({}, { locale })}
          type="password"
          name="password"
          autoComplete="current-password"
          readOnly={held}
          value={password}
          onChange={(event) => {
            on.password(event.target.value);
          }}
        />
      )}
    </>
  );
}

// The consent route on the first screen, when the instance can start one.
function SignInButtons({ locale, signInProviders, held, on }: AddressFieldsProps) {
  if (signInProviders.length === 0) {
    return <p className={styles.note}>{m.account_no_providers({}, { locale })}</p>;
  }
  return (
    <>
      <p className={styles.note}>{m.account_or({}, { locale })}</p>
      {signInProviders.map((signIn) => (
        <Button
          key={signIn}
          type="button"
          held={held}
          onClick={() => {
            on.signIn(signIn);
          }}
        >
          {m.account_continue_with({ provider: signInName(signIn) }, { locale })}
        </Button>
      ))}
    </>
  );
}

// The first screen and its two outcomes: the address with an optional
// password, the wait for discovery and the offer of manual entry.
export function AddressStep(props: AddressStepProps) {
  const { locale, step, address, retryRemaining, on } = props;
  const detecting = step === "detecting";
  const held = detecting || retryRemaining !== null;
  return (
    <form
      className={styles.form}
      onSubmit={(event) => {
        event.preventDefault();
        if (!held) {
          on.continue();
        }
      }}
    >
      <AddressFields {...props} held={held} />
      {detecting && (
        <output className={styles.status}>{m.account_detecting({ address }, { locale })}</output>
      )}
      {step === "notFound" && <Callout tone="warn">{m.account_not_found({}, { locale })}</Callout>}
      <Button
        variant="primary"
        className={styles.submit}
        type="submit"
        held={held}
        pending={detecting}
      >
        {submitLabel(locale, step, retryRemaining)}
      </Button>
      {step === "notFound" && (
        <Button type="button" onClick={on.enterDetails}>
          {m.account_enter_details({}, { locale })}
        </Button>
      )}
      {step === "typing" && <SignInButtons {...props} held={held} />}
    </form>
  );
}
