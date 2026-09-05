// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useEffect, useRef, useState } from "react";

import { Button } from "../../design-system/button";
import { DialogActions } from "../../design-system/dialog";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import styles from "./issued-secret.module.css";

interface IssuedSecretProps {
  locale: Locale;
  secret: string;
  onDone: () => void;
}

type CopyState = "idle" | "copied" | "failed";

function copyStateText(state: CopyState, locale: Locale): string {
  if (state === "copied") {
    return m.users_copied({}, { locale });
  }
  return state === "failed" ? m.users_copy_failed({}, { locale }) : "";
}

// The one-time password, shown once. Nothing here keeps it: the parent
// drops it when the dialog has closed.
export function IssuedSecret({ locale, secret, onDone }: IssuedSecretProps) {
  const [copyState, setCopyState] = useState<CopyState>("idle");
  const copyButton = useRef<HTMLButtonElement>(null);
  // The confirm buttons left with the previous face; the copy comes first.
  useEffect(() => {
    copyButton.current?.focus();
  }, []);
  const copy = async (): Promise<void> => {
    try {
      await navigator.clipboard.writeText(secret);
      setCopyState("copied");
    } catch {
      setCopyState("failed");
    }
  };
  return (
    <>
      <div className={styles.row}>
        <output className={styles.secret} aria-label={m.users_one_time_password({}, { locale })}>
          {secret}
        </output>
        <Button
          ref={copyButton}
          onClick={() => {
            void copy();
          }}
        >
          {m.users_copy({}, { locale })}
        </Button>
      </div>
      <output className={copyState === "failed" ? styles.failed : styles.state}>
        {copyStateText(copyState, locale)}
      </output>
      <DialogActions>
        <Button variant="primary" onClick={onDone}>
          {m.users_done({}, { locale })}
        </Button>
      </DialogActions>
    </>
  );
}
