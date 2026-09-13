// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { fitsAddress } from "@huliho/core";
import type { AccountRow, AccountsFailureCode, FoundServer } from "@huliho/core";
import { useId, useState } from "react";

import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { AddressStep } from "./address-step";
import type { AddressStepName } from "./address-step";
import { Callout } from "./callout";
import { ConfirmHostStep } from "./confirm-host-step";
import { CredentialStep } from "./credential-step";
import type { FieldStep, Step } from "./flow";
import { InsecureStep } from "./insecure-step";
import { ManualStep } from "./manual-step";
import { initialManual } from "./manual-target";
import type { ManualValues } from "./manual-target";
import { credentialKindOf, credentialOf } from "./presets";
import type { AddAccountFlow } from "./use-add-account";
import styles from "./add-account.module.css";

export interface AddAccountCardProps {
  locale: Locale;
  flow: AddAccountFlow;
  online: boolean;
}

// What the user typed, kept across the steps that leave and return.
interface Fields {
  password: string;
  // The credential of the found step; null falls back to the password.
  secret: string | null;
  manual: ManualValues;
}

interface StepViewProps extends AddAccountCardProps {
  fields: Fields;
  patch: (part: Partial<Fields>) => void;
}

interface Waiting {
  pending: boolean;
}

function isAddressStep(step: Step): step is Extract<Step, { name: AddressStepName }> {
  return step.name === "typing" || step.name === "detecting" || step.name === "notFound";
}

// The server a refusal names; null on a reconnect, where the row has no
// server name to check.
function hostOf(step: Step, fields: Fields): string | null {
  if (step.name === "found" || step.name === "confirmHost") {
    return step.found.host;
  }
  if (step.name === "reconnect" || (step.name === "connecting" && step.from.name === "reconnect")) {
    return null;
  }
  return step.name === "connecting" ? step.host : fields.manual.server.trim();
}

// The row a reconnect refusal names, when the step is one.
function rowNameOf(step: Step): string {
  if (step.name === "reconnect") {
    return step.account.name;
  }
  return step.name === "connecting" && step.from.name === "reconnect" ? step.from.account.name : "";
}

// A server that did not answer as one: the host to check where there is
// one, the row's name where there is none.
function serverText(
  failure: AccountsFailureCode,
  host: string | null,
  name: string,
  locale: Locale,
): string {
  if (host === null) {
    return m.account_error_server_again({ name }, { locale });
  }
  return failure === "upstream_unreachable"
    ? m.account_error_unreachable({ host }, { locale })
    : m.account_error_unsupported({ host }, { locale });
}

// The refusals no field owns, said above the fields.
function noticeText(
  failure: AccountsFailureCode | null,
  step: Step,
  fields: Fields,
  locale: Locale,
): string | null {
  switch (failure) {
    case "rate_limited":
      return m.account_error_rate_limited({}, { locale });
    case "unavailable":
      return m.signin_error_unavailable({}, { locale });
    case "upstream_unreachable":
    case "upstream_unsupported":
      return serverText(failure, hostOf(step, fields), rowNameOf(step), locale);
    case "smtp_auth_unavailable":
      return m.account_error_smtp_auth({}, { locale });
    case "invalid_request":
      return m.account_error_invalid({}, { locale });
    case "not_found":
      return m.account_error_gone({}, { locale });
    default:
      return null;
  }
}

function AddressView({
  locale,
  flow,
  fields,
  patch,
  step,
}: StepViewProps & { step: AddressStepName }) {
  return (
    <AddressStep
      locale={locale}
      step={step}
      address={flow.state.address}
      password={fields.password}
      failure={flow.state.failure}
      retryRemaining={flow.retryRemaining}
      on={{
        address: (value) => {
          flow.dispatch({ type: "typed", address: value });
        },
        password: (value) => {
          patch({ password: value });
        },
        continue: () => {
          if (fitsAddress(flow.state.address)) {
            flow.detect();
          } else {
            flow.dispatch({ type: "failed", failure: "invalid_request" });
          }
        },
        enterDetails: () => {
          flow.dispatch({ type: "enterDetails" });
        },
      }}
    />
  );
}

function ConfirmView({
  locale,
  flow,
  fields,
  found,
  pending,
}: StepViewProps & Waiting & { found: FoundServer }) {
  return (
    <ConfirmHostStep
      locale={locale}
      address={flow.state.address}
      found={found}
      pending={pending}
      retryRemaining={flow.retryRemaining}
      onConnect={() => {
        flow.connect({
          provider: found.provider,
          target: found.target,
          credential: credentialOf(found.credentialKind, fields.secret ?? fields.password),
          host: found.host,
        });
      }}
      onDifferentServer={() => {
        flow.dispatch({ type: "differentServer" });
      }}
    />
  );
}

function ManualView({ locale, flow, fields, patch, pending }: StepViewProps & Waiting) {
  return (
    <ManualStep
      locale={locale}
      address={flow.state.address}
      values={fields.manual}
      failure={flow.state.failure}
      pending={pending}
      retryRemaining={flow.retryRemaining}
      onChange={(part) => {
        flow.dispatch({ type: "edited" });
        patch({ manual: { ...fields.manual, ...part } });
      }}
      onConnect={(built) => {
        flow.connect({
          provider: "generic",
          target: built.target,
          credential: { kind: "password", password: fields.manual.password },
          host: built.host,
        });
      }}
    />
  );
}

// The server found: the credential of its kind, with the typed password
// as the start for a password server.
function FoundView({
  locale,
  flow,
  fields,
  patch,
  found,
  pending,
}: StepViewProps & Waiting & { found: FoundServer }) {
  const kind = found.credentialKind;
  return (
    <CredentialStep
      locale={locale}
      address={flow.state.address}
      provider={found.provider}
      credentialKind={kind}
      host={found.host}
      found={{
        host: found.host,
        onChange: () => {
          patch({ secret: null });
          flow.dispatch({ type: "change" });
        },
      }}
      secret={fields.secret ?? (kind === "password" ? fields.password : "")}
      failure={flow.state.failure}
      pending={pending}
      retryRemaining={flow.retryRemaining}
      onSecret={(value) => {
        flow.dispatch({ type: "edited" });
        patch({ secret: value });
      }}
      onSubmit={() => {
        flow.dispatch({ type: "continue" });
      }}
    />
  );
}

// The row to reconnect: the credential of its kind, sent to the row.
function ReconnectView({
  locale,
  flow,
  fields,
  patch,
  account,
  pending,
}: StepViewProps & Waiting & { account: AccountRow }) {
  const kind = credentialKindOf(account.provider, account.authMethod);
  const secret = fields.secret ?? "";
  return (
    <CredentialStep
      locale={locale}
      address={flow.state.address}
      provider={account.provider}
      credentialKind={kind}
      host={account.name}
      found={null}
      secret={secret}
      failure={flow.state.failure}
      pending={pending}
      retryRemaining={flow.retryRemaining}
      onSecret={(value) => {
        flow.dispatch({ type: "edited" });
        patch({ secret: value });
      }}
      onSubmit={() => {
        flow.reconnect(credentialOf(kind, secret));
      }}
    />
  );
}

// The steps with a credential field, waiting or not.
function FieldView({ step, ...props }: StepViewProps & Waiting & { step: FieldStep }) {
  switch (step.name) {
    case "found":
      return <FoundView {...props} found={step.found} />;
    case "reconnect":
      return <ReconnectView {...props} account={step.account} />;
    default:
      return <ManualView {...props} />;
  }
}

function StepView(props: StepViewProps) {
  const { locale, flow } = props;
  const { step } = flow.state;
  if (isAddressStep(step)) {
    return <AddressView {...props} step={step.name} />;
  }
  if (step.name === "confirmHost") {
    return <ConfirmView {...props} found={step.found} pending={false} />;
  }
  if (step.name === "connecting" && step.from.name === "found") {
    return <ConfirmView {...props} found={step.from.found} pending />;
  }
  if (step.name === "connecting") {
    return <FieldView {...props} step={step.from} pending />;
  }
  if (step.name === "insecure") {
    return (
      <InsecureStep
        locale={locale}
        onBack={() => {
          flow.dispatch({ type: "back" });
        }}
      />
    );
  }
  if (step.name === "connected") {
    return null;
  }
  return <FieldView {...props} step={step} pending={false} />;
}

export function AddAccountCard({ locale, flow, online }: AddAccountCardProps) {
  const titleId = useId();
  const { step, failure } = flow.state;
  const [fields, setFields] = useState<Fields>(() => ({
    password: "",
    secret: null,
    manual: initialManual(),
  }));
  const patch = (part: Partial<Fields>): void => {
    setFields((current) => ({ ...current, ...part }));
  };
  const title =
    step.name === "reconnect"
      ? m.account_reconnect_title({ name: step.account.name }, { locale })
      : m.account_title({}, { locale });
  // The address field owns a malformed address; every other refusal is a callout.
  const owned = isAddressStep(step) && failure === "invalid_request";
  const notice = noticeText(owned ? null : failure, step, fields, locale);
  return (
    <section className={styles.card} data-step={step.name} aria-labelledby={titleId}>
      <h1 id={titleId} className={styles.title}>
        {title}
      </h1>
      {step.name === "typing" && <p className={styles.lead}>{m.account_lead({}, { locale })}</p>}
      {!online && (
        <Callout tone="warn" live="status">
          {m.account_offline({}, { locale })}
        </Callout>
      )}
      {notice !== null && (
        <Callout tone="danger" live="alert">
          {notice}
        </Callout>
      )}
      <StepView locale={locale} flow={flow} online={online} fields={fields} patch={patch} />
    </section>
  );
}
