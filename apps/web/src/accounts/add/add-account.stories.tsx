// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow, FoundServer } from "@huliho/core";
import type { Meta, StoryObj } from "@storybook/react-vite";
import type { JSX } from "react";

import { AddAccountCard } from "./add-account-card";
import type { FlowState, Step } from "./flow";
import type { AddAccountFlow } from "./use-add-account";
import styles from "./add-account.module.css";

const ADDRESS = "sanne@fastmail.com";
const GENERIC_ADDRESS = "sanne@dekker-mail.nl";
// Screenshots must not age, so the row sits at fixed distances from a fixed now.
const NOW = new Date("2026-05-14T10:00:00").getTime();
const HOUR_MS = 3_600_000;
const DAY_MS = 24 * HOUR_MS;
const RETRY_SECONDS = 872;

const FASTMAIL: FoundServer = {
  provider: "fastmail",
  kind: "jmap",
  target: { kind: "jmap", sessionUrl: "https://api.fastmail.com/jmap/session" },
  credentialKind: "apiToken",
  host: "api.fastmail.com",
  oauthAvailable: false,
};
const GMAIL: FoundServer = {
  provider: "gmail",
  kind: "imap",
  target: {
    kind: "imap",
    username: "sanne@gmail.com",
    imap: { host: "imap.gmail.com", port: 993, tls: "implicit" },
    smtp: { host: "smtp.gmail.com", port: 465, tls: "implicit" },
  },
  credentialKind: "appPassword",
  host: "imap.gmail.com",
  oauthAvailable: false,
};
const GENERIC: FoundServer = {
  provider: "generic",
  kind: "imap",
  target: {
    kind: "imap",
    username: GENERIC_ADDRESS,
    imap: { host: "imap.dekker-mail.nl", port: 993, tls: "implicit" },
    smtp: { host: "smtp.dekker-mail.nl", port: 465, tls: "implicit" },
  },
  credentialKind: "password",
  host: "imap.dekker-mail.nl",
  oauthAvailable: false,
};
const MICROSOFT: FoundServer = {
  provider: "microsoft",
  kind: "imap",
  target: {
    kind: "imap",
    username: "sanne@outlook.com",
    imap: { host: "outlook.office365.com", port: 993, tls: "implicit" },
    smtp: { host: "smtp.office365.com", port: 587, tls: "starttls" },
  },
  credentialKind: "oauth",
  host: "outlook.office365.com",
  oauthAvailable: false,
};
const ROW: AccountRow = {
  id: "acc-1",
  address: ADDRESS,
  name: "Fastmail",
  provider: "fastmail",
  kind: "jmap",
  authMethod: "bearer",
  stoppedCause: "credentials",
  stoppedAt: NOW - HOUR_MS,
  createdAt: NOW - DAY_MS,
};

function nothing(): void {
  // Stories render states; nothing runs.
}

function frozen(state: FlowState, retryRemaining: number | null): AddAccountFlow {
  return {
    state,
    retryRemaining,
    dispatch: nothing,
    detect: nothing,
    connect: nothing,
    reconnect: nothing,
  };
}

function at(step: Step, address = ADDRESS, failure: FlowState["failure"] = null): FlowState {
  return { step, address, failure };
}

interface CardProps {
  state: FlowState;
  online?: boolean;
  retryRemaining?: number | null;
}

function Card({ state, online = true, retryRemaining = null }: CardProps): JSX.Element {
  return (
    <div className={styles.page}>
      <div className={styles.screen}>
        <AddAccountCard locale="en" flow={frozen(state, retryRemaining)} online={online} />
      </div>
    </div>
  );
}

const meta: Meta = {
  title: "Accounts/Add account",
};

export default meta;

export const Default: StoryObj = {
  render: () => <Card state={at({ name: "typing" }, "")} />,
};

export const Detecting: StoryObj = {
  render: () => <Card state={at({ name: "detecting" })} />,
};

export const FoundFastmail: StoryObj = {
  render: () => <Card state={at({ name: "found", found: FASTMAIL })} />,
};

export const FoundGmail: StoryObj = {
  render: () => <Card state={at({ name: "found", found: GMAIL }, "sanne@gmail.com")} />,
};

export const FoundGeneric: StoryObj = {
  render: () => <Card state={at({ name: "found", found: GENERIC }, GENERIC_ADDRESS)} />,
};

export const FoundMicrosoft: StoryObj = {
  render: () => <Card state={at({ name: "found", found: MICROSOFT }, "sanne@outlook.com")} />,
};

export const ConfirmHost: StoryObj = {
  render: () => <Card state={at({ name: "confirmHost", found: FASTMAIL })} />,
};

export const Connecting: StoryObj = {
  render: () => (
    <Card
      state={at({
        name: "connecting",
        from: { name: "found", found: FASTMAIL },
        host: FASTMAIL.host,
      })}
    />
  ),
};

export const NotFound: StoryObj = {
  render: () => <Card state={at({ name: "notFound" }, GENERIC_ADDRESS)} />,
};

export const Manual: StoryObj = {
  render: () => <Card state={at({ name: "manual" }, GENERIC_ADDRESS)} />,
};

export const Insecure: StoryObj = {
  render: () => (
    <Card state={at({ name: "insecure", from: { name: "manual" } }, GENERIC_ADDRESS)} />
  ),
};

export const WrongCredentials: StoryObj = {
  render: () => (
    <Card state={at({ name: "found", found: FASTMAIL }, ADDRESS, "upstream_credentials")} />
  ),
};

export const Unreachable: StoryObj = {
  render: () => (
    <Card state={at({ name: "found", found: GENERIC }, GENERIC_ADDRESS, "upstream_unreachable")} />
  ),
};

export const SmtpAuthUnavailable: StoryObj = {
  render: () => (
    <Card state={at({ name: "found", found: GENERIC }, GENERIC_ADDRESS, "smtp_auth_unavailable")} />
  ),
};

export const RateLimited: StoryObj = {
  render: () => (
    <Card state={at({ name: "typing" }, ADDRESS, "rate_limited")} retryRemaining={RETRY_SECONDS} />
  ),
};

export const Offline: StoryObj = {
  render: () => <Card state={at({ name: "typing" })} online={false} />,
};

export const Reconnect: StoryObj = {
  render: () => <Card state={at({ name: "reconnect", account: ROW })} />,
};
