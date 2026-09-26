// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow } from "@huliho/core";
import { Link } from "@tanstack/react-router";
import { CircleAlert, CirclePause } from "lucide-react";
import { useEffect, useRef } from "react";
import type { Ref, RefObject } from "react";

import { RetryButton } from "../accounts/retry-button";
import type { RetryOutcome } from "../accounts/use-retry-account";
import buttonStyles from "../design-system/button.module.css";
import { cx } from "../design-system/cx";
import spoken from "../design-system/spoken.module.css";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { useStatusText } from "./status-text";
import { markedFromMailbox } from "./thread-history";
import { useLingering } from "./use-lingering";
import styles from "./thread-list-banner.module.css";

interface ThreadListBannerProps {
  locale: Locale;
  account: AccountRow;
  // How often the server checks a stopped account, as the list answered.
  probeIntervalMinutes: number;
  // The offline strip takes the slot while the device is offline.
  online: boolean;
  // What the last retry of this account did; nothing for one never retried.
  outcome: RetryOutcome | undefined;
  onRetry: () => void;
  // A retry the user pressed turned the stop into an expired one, so
  // Reconnect takes the focus its button left with.
  takeFocus: boolean;
  onFocusTaken: () => void;
  // The banner's box, so a caller can tell whether its action holds the focus.
  ref?: Ref<HTMLDivElement> | undefined;
}

// Expired waits for the user to sign in again; stopped waits for the
// probe or a retry.
type Cause = "expired" | "stopped";

function causeOf(account: AccountRow, online: boolean): Cause | null {
  if (!online || account.stoppedCause === null) {
    return null;
  }
  return account.stoppedCause === "credentials" ? "expired" : "stopped";
}

// The sentence of the cause, or what the last retry did while the stop stands.
function sentenceOf(props: ThreadListBannerProps, cause: Cause): string {
  const { locale, outcome, probeIntervalMinutes: minutes } = props;
  if (cause === "expired") {
    return m.mail_banner_expired({ address: props.account.address }, { locale });
  }
  if (outcome === "failed") {
    return m.accounts_retry_failed({}, { locale });
  }
  return outcome === "stillStopped"
    ? m.accounts_still_stopped({ minutes }, { locale })
    : m.accounts_stopped({ minutes }, { locale });
}

// What the region says: the sentence of the cause, the word of a pass
// while the account runs and nothing while the banner is idle.
function spokenOf(props: ThreadListBannerProps, cause: Cause | null): string {
  if (cause !== null) {
    return sentenceOf(props, cause);
  }
  const passed = props.outcome === "resumed" && props.account.stoppedCause === null;
  return passed ? m.accounts_resumed({}, { locale: props.locale }) : "";
}

// Retry leaves with a credential now rejected; when it took the cursor
// along, Reconnect takes it, so it is never lost. Only a retry the user
// pressed asks for that, never a turn the probe made.
function useReconnectFocus(cause: Cause | null, takeFocus: boolean, onFocusTaken: () => void) {
  const linkRef = useRef<HTMLAnchorElement>(null);
  useEffect(() => {
    if (!takeFocus) {
      return;
    }
    if (cause === "expired" && document.activeElement === document.body) {
      linkRef.current?.focus();
    }
    onFocusTaken();
  }, [cause, takeFocus, onFocusTaken]);
  return linkRef;
}

interface ActionProps extends ThreadListBannerProps {
  cause: Cause | null;
  linkRef: RefObject<HTMLAnchorElement | null>;
}

// The one way out: Reconnect to the card for an expired connection,
// Retry for a server that could not be reached.
function Action({ cause, linkRef, locale, account, outcome, onRetry }: ActionProps) {
  if (cause === "expired") {
    return (
      <Link
        ref={linkRef}
        to="/accounts/new"
        search={{ reconnect: account.id }}
        state={markedFromMailbox}
        className={cx(buttonStyles.button, buttonStyles.primary)}
        aria-label={m.accounts_reconnect_for({ name: account.name }, { locale })}
      >
        {m.accounts_reconnect({}, { locale })}
      </Link>
    );
  }
  if (cause === "stopped") {
    return (
      <RetryButton
        locale={locale}
        name={account.name}
        pending={outcome === "pending"}
        onRetry={onRetry}
      />
    );
  }
  return null;
}

// The banner over the list of a stopped account: the cause as one
// sentence with its one way out. The status region is always in the
// DOM and the sentence lands in it once the banner renders; the
// outcome of a retry replaces it in place. A pass says so there while
// the banner fades, its action gone at once.
export function ThreadListBanner({ ref, ...props }: ThreadListBannerProps) {
  const cause = causeOf(props.account, props.online);
  const { shown, leaving } = useLingering(cause);
  const sentenceRef = useRef<HTMLParagraphElement>(null);
  const linkRef = useReconnectFocus(cause, props.takeFocus, props.onFocusTaken);
  useStatusText(sentenceRef, spokenOf(props, cause));
  const Icon = shown === "expired" ? CircleAlert : CirclePause;
  return (
    <div
      ref={ref}
      className={styles.banner}
      data-cause={shown ?? undefined}
      data-idle={shown === null || undefined}
      data-leaving={leaving || undefined}
    >
      {shown !== null && <Icon className={styles.icon} aria-hidden="true" />}
      <p
        ref={sentenceRef}
        role="status"
        className={cx(styles.sentence, cause === null ? spoken.spoken : undefined)}
      />
      {leaving && shown !== null && (
        <span aria-hidden="true" className={styles.sentence}>
          {sentenceOf(props, shown)}
        </span>
      )}
      <Action {...props} cause={cause} linkRef={linkRef} />
    </div>
  );
}
