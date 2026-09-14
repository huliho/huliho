// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { SignInProvider } from "@huliho/core";
import { useEffect, useRef } from "react";

import { Button } from "../../design-system/button";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { Callout } from "./callout";
import { signInName } from "./presets";
import styles from "./add-account.module.css";

interface ConsentStepProps {
  locale: Locale;
  signIn: SignInProvider;
  // Whether the provider's window is open; a closed one gets a button.
  opened: boolean;
  // True until the start request answered, so there is nowhere to open yet.
  starting: boolean;
  onOpen: () => void;
  onCancel: () => void;
}

// The wait for the provider's window: the page finishes by itself, so
// the one action is to give up. Google's testing mode gets its note.
export function ConsentStep({
  locale,
  signIn,
  opened,
  starting,
  onOpen,
  onCancel,
}: ConsentStepProps) {
  const open = useRef<HTMLButtonElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  // The first control: the open button while the window is closed, Cancel
  // once it opened and that button left.
  useEffect(() => {
    (opened ? cancel.current : open.current)?.focus();
  }, [opened]);
  const provider = signInName(signIn);
  return (
    <div className={styles.form}>
      <Callout tone="info" live="status">
        {opened
          ? m.account_consent_open({ provider }, { locale })
          : m.account_consent_blocked({ provider }, { locale })}
      </Callout>
      {signIn === "google" && (
        <p className={styles.lead}>{m.account_consent_testing_note({}, { locale })}</p>
      )}
      {!opened && (
        <Button
          ref={open}
          variant="primary"
          className={styles.submit}
          held={starting}
          onClick={onOpen}
        >
          {m.account_consent_open_window({ provider }, { locale })}
        </Button>
      )}
      <Button ref={cancel} onClick={onCancel}>
        {m.cancel_action({}, { locale })}
      </Button>
    </div>
  );
}
