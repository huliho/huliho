// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Meta, StoryObj } from "@storybook/react-vite";
import type { JSX } from "react";

import { LegalNotices } from "../legal/legal-notices";
import { ChoosePasswordCard } from "./choose-password";
import { SignInForm } from "./sign-in-form";
import styles from "./sign-in.module.css";

const RETRY_SECONDS = 872;

function noSubmit(): void {
  // Stories render states; nothing submits.
}

function Framed({ children }: { children: JSX.Element }): JSX.Element {
  return (
    <div className={styles.screen}>
      <div className={styles.column}>{children}</div>
    </div>
  );
}

const meta: Meta = {
  title: "Sign-in/Form",
};

export default meta;

export const Default: StoryObj = {
  render: () => (
    <Framed>
      <SignInForm
        locale="en"
        pending={false}
        failure={null}
        retryRemaining={null}
        onSubmit={noSubmit}
      />
    </Framed>
  ),
};

export const SigningIn: StoryObj = {
  render: () => (
    <Framed>
      <SignInForm locale="en" pending failure={null} retryRemaining={null} onSubmit={noSubmit} />
    </Framed>
  ),
};

export const WrongCredentials: StoryObj = {
  render: () => (
    <Framed>
      <SignInForm
        locale="en"
        pending={false}
        failure="invalid_credentials"
        retryRemaining={null}
        onSubmit={noSubmit}
      />
    </Framed>
  ),
};

export const TooManyAttempts: StoryObj = {
  render: () => (
    <Framed>
      <SignInForm
        locale="en"
        pending={false}
        failure="rate_limited"
        retryRemaining={RETRY_SECONDS}
        onSubmit={noSubmit}
      />
    </Framed>
  ),
};

export const ServerUnreachable: StoryObj = {
  render: () => (
    <Framed>
      <SignInForm
        locale="en"
        pending={false}
        failure="unavailable"
        retryRemaining={null}
        onSubmit={noSubmit}
      />
    </Framed>
  ),
};

export const ChoosePassword: StoryObj = {
  render: () => (
    <Framed>
      <ChoosePasswordCard
        locale="en"
        pending={false}
        failure={null}
        retryRemaining={null}
        onSubmit={noSubmit}
        onSignOut={noSubmit}
      />
    </Framed>
  ),
};

export const Notices: StoryObj = {
  render: () => (
    <Framed>
      <LegalNotices locale="en" />
    </Framed>
  ),
};
