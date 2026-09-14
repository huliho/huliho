// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { fitsAddress } from "@huliho/core";
import type { AccountRow, FoundServer, SignInProvider } from "@huliho/core";
import { useId, useState } from "react";

import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { AddressStep } from "./address-step";
import type { AddressStepName } from "./address-step";
import { Callout } from "./callout";
import { ConfirmHostStep } from "./confirm-host-step";
import { ConsentDeniedStep } from "./consent-denied-step";
import { ConsentStep } from "./consent-step";
import { CredentialStep } from "./credential-step";
import type { SignInOffer } from "./credential-step";
import type {
  ConsentDeniedStep as DeniedState,
  ConsentStep as ConsentState,
  FieldStep,
  Step,
} from "./flow";
import { InsecureStep } from "./insecure-step";
import { ManualStep } from "./manual-step";
import { initialManual } from "./manual-target";
import type { ManualValues } from "./manual-target";
import { noticeText } from "./notice";
import { credentialKindOf, credentialOf, signInProviderOf } from "./presets";
import type { AddAccountFlow } from "./use-add-account";
import styles from "./add-account.module.css";

export interface AddAccountCardProps {
  locale: Locale;
  flow: AddAccountFlow;
  online: boolean;
  // The sign-in providers the instance can start a consent with.
  signInProviders: SignInProvider[];
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

// The row whose reconnect the card is on, through its consent and its connect.
function reconnectRow(step: Step): AccountRow | null {
  switch (step.name) {
    case "reconnect":
      return step.account;
    case "connecting":
    case "consent":
    case "consentDenied":
      return step.from.name === "reconnect" ? step.from.account : null;
    default:
      return null;
  }
}

// The address goes out only when it fits; a typo marks the field.
function withAddress(flow: AddAccountFlow, then: () => void): void {
  if (fitsAddress(flow.state.address)) {
    then();
  } else {
    flow.dispatch({ type: "failed", failure: "invalid_request" });
  }
}

// The consent route beside a credential step, when the instance can start it.
function signInOffer(
  signIn: SignInProvider | null,
  available: boolean,
  flow: AddAccountFlow,
): SignInOffer | null {
  if (signIn === null || !available) {
    return null;
  }
  return {
    provider: signIn,
    onStart: () => {
      flow.startConsent(signIn);
    },
  };
}

function AddressView({
  locale,
  flow,
  fields,
  patch,
  signInProviders,
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
      signInProviders={signInProviders}
      on={{
        address: (value) => {
          flow.dispatch({ type: "typed", address: value });
        },
        password: (value) => {
          patch({ password: value });
        },
        continue: () => {
          withAddress(flow, flow.detect);
        },
        signIn: (signIn) => {
          withAddress(flow, () => {
            flow.startConsent(signIn);
          });
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
// as the start for a password server and the consent where it has one.
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
      signIn={signInOffer(signInProviderOf(found.provider), found.oauthAvailable, flow)}
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

// The row to reconnect: the credential of its kind sent to the row; a
// row that signs in through a consent gets the consent again.
function ReconnectView({
  locale,
  flow,
  fields,
  patch,
  signInProviders,
  account,
  pending,
}: StepViewProps & Waiting & { account: AccountRow }) {
  const kind = credentialKindOf(account.provider, account.authMethod);
  const secret = fields.secret ?? "";
  const signIn = signInProviderOf(account.provider);
  const consent = kind === "oauth" && signIn !== null && signInProviders.includes(signIn);
  return (
    <CredentialStep
      locale={locale}
      address={flow.state.address}
      provider={account.provider}
      credentialKind={kind}
      host={account.name}
      found={null}
      signIn={signInOffer(signIn, consent, flow)}
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

function ConsentView({ locale, flow, step }: StepViewProps & { step: ConsentState }) {
  return (
    <ConsentStep
      locale={locale}
      signIn={step.signIn}
      opened={step.opened}
      starting={step.id === null}
      onOpen={flow.openConsentWindow}
      onCancel={flow.cancelConsent}
    />
  );
}

function ConsentDeniedView({ locale, flow, step }: StepViewProps & { step: DeniedState }) {
  return (
    <ConsentDeniedStep
      locale={locale}
      signIn={step.signIn}
      address={flow.state.address}
      cause={step.cause}
      passwordOffered={step.from.name !== "reconnect"}
      onRetry={() => {
        flow.startConsent(step.signIn);
      }}
      onUsePassword={flow.usePassword}
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

// The steps without a credential field.
function PlainView(props: StepViewProps) {
  const { locale, flow } = props;
  const { step } = flow.state;
  switch (step.name) {
    case "typing":
    case "detecting":
    case "notFound":
      return <AddressView {...props} step={step.name} />;
    case "confirmHost":
      return <ConfirmView {...props} found={step.found} pending={false} />;
    case "insecure":
      return (
        <InsecureStep
          locale={locale}
          onBack={() => {
            flow.dispatch({ type: "back" });
          }}
        />
      );
    case "consent":
      return <ConsentView {...props} step={step} />;
    case "consentDenied":
      return <ConsentDeniedView {...props} step={step} />;
    default:
      return null;
  }
}

function StepView(props: StepViewProps) {
  const { step } = props.flow.state;
  if (step.name === "connecting") {
    return step.from.name === "found" ? (
      <ConfirmView {...props} found={step.from.found} pending />
    ) : (
      <FieldView {...props} step={step.from} pending />
    );
  }
  if (step.name === "found" || step.name === "manual" || step.name === "reconnect") {
    return <FieldView {...props} step={step} pending={false} />;
  }
  return <PlainView {...props} />;
}

export function AddAccountCard(props: AddAccountCardProps) {
  const { locale, flow, online } = props;
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
  const row = reconnectRow(step);
  const title =
    row === null
      ? m.account_title({}, { locale })
      : m.account_reconnect_title({ name: row.name }, { locale });
  // The address field owns a malformed address; every other refusal is a callout.
  const owned = isAddressStep(step) && failure === "invalid_request";
  const notice = noticeText(owned ? null : failure, step, fields.manual.server, locale);
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
      <StepView {...props} fields={fields} patch={patch} />
    </section>
  );
}
