// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Image, ImageOff, Shield } from "lucide-react";
import { useId, useRef, useState } from "react";

import { LeavingButton, focusHeir } from "../design-system/button";
import spoken from "../design-system/spoken.module.css";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { useStatusText } from "./status-text";
import type { RemoteBar } from "./use-remote-content";
import styles from "./remote-content-bar.module.css";

export interface RemoteContentBarProps {
  locale: Locale;
  state: RemoteBar;
  // The sender the grant is keyed on, named in the standing state.
  sender: string | null;
  // Whether Always for this sender is offered.
  canAlways: boolean;
  // Whether a grant is on its way to the server.
  pending: boolean;
  onLoadOnce: () => void;
  onAllow: () => void;
  onStop: () => void;
}

function sentenceOf(state: RemoteBar, sender: string | null, locale: Locale): string {
  switch (state) {
    case "once":
      return m.remote_loaded_once({}, { locale });
    case "always":
      return m.remote_always_on({ sender: sender ?? "" }, { locale });
    case "failed":
      return m.remote_failed_check({}, { locale });
    default:
      return m.remote_blocked({}, { locale });
  }
}

// The icon tells the states apart where the sentence is read later: a
// crossed image for blocked, a shield for a message that failed the
// server's check, an image for one that loads.
function iconOf(state: RemoteBar) {
  if (state === "failed") {
    return <Shield className={styles.icon} aria-hidden="true" />;
  }
  return state === "blocked" ? (
    <ImageOff className={styles.icon} aria-hidden="true" />
  ) : (
    <Image className={styles.icon} aria-hidden="true" />
  );
}

// The strip above a message that names remote images: what happens to
// them and the reader's choices. A choice leaves once taken; the bar takes
// its focus and the status region, empty until then, says what changed.
export function RemoteContentBar(props: RemoteContentBarProps) {
  const { locale, state, sender, canAlways, pending, onLoadOnce, onAllow, onStop } = props;
  const sentence = sentenceOf(state, sender, locale);
  const sentenceId = useId();
  const regionRef = useRef<HTMLSpanElement>(null);
  const [chosen, setChosen] = useState(false);
  useStatusText(regionRef, chosen ? sentence : "");
  const taking = (choice: () => void) => (): void => {
    setChosen(true);
    choice();
  };
  const offersOnce = state === "blocked" || state === "failed";
  return (
    <div
      {...focusHeir}
      role="group"
      aria-describedby={sentenceId}
      className={styles.bar}
      data-state={state}
    >
      {iconOf(state)}
      <p id={sentenceId} className={styles.sentence}>
        {sentence}
      </p>
      <span ref={regionRef} role="status" className={spoken.spoken} />
      {(offersOnce || state === "always") && (
        <div className={styles.actions}>
          {offersOnce && (
            <LeavingButton onClick={taking(onLoadOnce)}>
              {m.remote_load_once({}, { locale })}
            </LeavingButton>
          )}
          {state === "blocked" && canAlways && (
            <LeavingButton variant="plain" pending={pending} onClick={taking(onAllow)}>
              {m.remote_always({}, { locale })}
            </LeavingButton>
          )}
          {state === "always" && (
            <LeavingButton variant="plain" pending={pending} onClick={taking(onStop)}>
              {m.remote_stop({}, { locale })}
            </LeavingButton>
          )}
        </div>
      )}
    </div>
  );
}
