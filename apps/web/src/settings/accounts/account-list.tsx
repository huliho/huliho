// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow } from "@huliho/core";
import { Link } from "@tanstack/react-router";
import { useEffect, useRef } from "react";
import type { RefObject } from "react";

import { RetryButton } from "../../accounts/retry-button";
import type { RetryOutcome, RetryOutcomes } from "../../accounts/use-retry-account";
import { Avatar } from "../../design-system/avatar";
import { Button } from "../../design-system/button";
import buttonStyles from "../../design-system/button.module.css";
import { cx } from "../../design-system/cx";
import rowList from "../../design-system/row-list.module.css";
import { useRowFocus } from "../../design-system/use-row-focus";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { captionOf } from "./caption";
import type { RowCaption } from "./caption";
import styles from "./account-list.module.css";

export interface AccountListProps {
  rows: AccountRow[];
  locale: Locale;
  // How often the server checks a stopped account, as the list answered.
  probeIntervalMinutes: number;
  // What the last retry per row did; nothing for a row never retried.
  outcomes: RetryOutcomes;
  onRetry: (id: string) => void;
  onRemove: (id: string) => void;
  // Where the cursor goes once the last row leaves.
  afterLast: RefObject<HTMLElement | null>;
}

interface ItemProps {
  row: AccountRow;
  locale: Locale;
  minutes: number;
  outcome: RetryOutcome | undefined;
  onRetry: (id: string) => void;
  onRemove: (id: string) => void;
}

// The Remove button of a row, for the focus handoff between rows.
function controlOf(id: string): string {
  return `[data-account="${CSS.escape(id)}"]`;
}

function toneClass(tone: RowCaption["tone"]): string | undefined {
  if (tone === "warn") {
    return styles.captionWarn;
  }
  return tone === "danger" ? styles.captionDanger : undefined;
}

// The state line. A pass is a status and a failed check an alert, so
// both are read out where the button was pressed.
function Caption({ caption }: { caption: RowCaption }) {
  const Text = caption.live === "status" ? "output" : "span";
  return (
    <Text
      role={caption.live === "alert" ? "alert" : undefined}
      className={cx(styles.caption, toneClass(caption.tone))}
    >
      {caption.text}
    </Text>
  );
}

// Reconnect for a stop the user resolves, Retry for one the server may.
function StateAction({ row, locale, outcome, onRetry }: ItemProps) {
  const name = row.name;
  if (row.stoppedCause === "credentials") {
    return (
      <Link
        to="/accounts/new"
        search={{ reconnect: row.id }}
        className={cx(buttonStyles.button, buttonStyles.secondary)}
        aria-label={m.accounts_reconnect_for({ name }, { locale })}
      >
        {m.accounts_reconnect({}, { locale })}
      </Link>
    );
  }
  if (row.stoppedCause === null) {
    return null;
  }
  return (
    <RetryButton
      locale={locale}
      name={name}
      pending={outcome === "pending"}
      onRetry={() => {
        onRetry(row.id);
      }}
    />
  );
}

function AccountItem(props: ItemProps) {
  const { row, locale, outcome } = props;
  const item = useRef<HTMLLIElement>(null);
  const caption = captionOf(row, outcome, props.minutes, locale);
  // Retry leaves with a pass or with a credential now rejected; when it
  // took the cursor along, the row takes it back, so it is never lost.
  useEffect(() => {
    const buttonLeft =
      outcome === "resumed" || (outcome === "stillStopped" && row.stoppedCause === "credentials");
    if (buttonLeft && document.activeElement === document.body) {
      item.current?.focus();
    }
  }, [outcome, row.stoppedCause]);
  return (
    <li ref={item} tabIndex={-1} className={cx(rowList.row, styles.row)}>
      <Avatar name={row.name} locale={locale} />
      <div className={cx(rowList.facts, styles.facts)}>
        <span className={styles.name}>{row.name}</span>
        <span className={styles.address}>{row.address}</span>
        {caption !== null && <Caption caption={caption} />}
      </div>
      <div className={styles.actions}>
        <StateAction {...props} />
        <Button
          aria-label={m.accounts_remove_for({ name: row.name }, { locale })}
          data-account={row.id}
          onClick={() => {
            props.onRemove(row.id);
          }}
        >
          {m.accounts_remove({}, { locale })}
        </Button>
      </div>
    </li>
  );
}

export function AccountList(props: AccountListProps) {
  const { rows, locale, outcomes, onRetry, onRemove } = props;
  const list = useRef<HTMLUListElement>(null);
  const focusBefore = useRowFocus(rows, list, controlOf, props.afterLast);
  // Every row has Remove, so any neighbor can take the cursor.
  const removeAndKeepFocus = (id: string): void => {
    focusBefore(id, () => true);
    onRemove(id);
  };
  return (
    <ul
      ref={list}
      className={rowList.list}
      tabIndex={-1}
      aria-label={m.accounts_heading({}, { locale })}
    >
      {rows.map((row) => (
        <AccountItem
          key={row.id}
          row={row}
          locale={locale}
          minutes={props.probeIntervalMinutes}
          outcome={outcomes[row.id]}
          onRetry={onRetry}
          onRemove={removeAndKeepFocus}
        />
      ))}
    </ul>
  );
}
