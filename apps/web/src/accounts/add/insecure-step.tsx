// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useEffect, useRef } from "react";

import { Button } from "../../design-system/button";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { Callout } from "./callout";
import styles from "./add-account.module.css";

interface InsecureStepProps {
  locale: Locale;
  onBack: () => void;
}

// The one refusal with no field to return to: nothing was sent and
// nothing will be until the server offers encryption.
export function InsecureStep({ locale, onBack }: InsecureStepProps) {
  const back = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    back.current?.focus();
  }, []);
  return (
    <div className={styles.form}>
      <Callout tone="danger" live="alert">
        {m.account_insecure({}, { locale })}
      </Callout>
      <p className={styles.lead}>{m.account_insecure_advice({}, { locale })}</p>
      <Button ref={back} onClick={onBack}>
        {m.account_back({}, { locale })}
      </Button>
    </div>
  );
}
