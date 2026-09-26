// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow, MailCache } from "@huliho/core";
import { ChevronDown } from "lucide-react";

import { Avatar } from "../design-system/avatar";
import { cx } from "../design-system/cx";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import styles from "./account-switcher.module.css";

export interface AccountSwitcherProps {
  locale: Locale;
  cache: MailCache;
  accounts: readonly AccountRow[];
  account: AccountRow;
  // The full row names the account; the avatar alone fits the rail.
  variant: "full" | "avatar";
  // Told when another account opens, so a sheet around the menu can close.
  onNavigate?: (() => void) | undefined;
}

// The one word after the name of a stopped account, in the role color
// of its cause: danger where only the user can act, warn where the
// probe may.
function Mark({ account, locale }: { account: AccountRow; locale: Locale }) {
  if (account.stoppedCause === null) {
    return null;
  }
  const expired = account.stoppedCause === "credentials";
  return (
    <span className={cx(styles.mark, expired ? styles.markDanger : styles.markWarn)}>
      {expired ? m.mail_mark_expired({}, { locale }) : m.mail_mark_stopped({}, { locale })}
    </span>
  );
}

// The name clips before the mark does, so the word that carries the
// state stays in view.
export function AccountFacts({ account, locale }: { account: AccountRow; locale: Locale }) {
  return (
    <span className={styles.facts}>
      <span className={styles.name}>
        <span className={styles.nameText}>{account.name}</span>
        <Mark account={account} locale={locale} />
      </span>
      <span className={styles.address}>{account.address}</span>
    </span>
  );
}

export function triggerClass(variant: AccountSwitcherProps["variant"]): string | undefined {
  return variant === "full" ? styles.trigger : styles.avatarTrigger;
}

// What the switcher's trigger shows: the avatar alone on the rail, the
// whole card with its mark in the sidebar.
export function AccountCard({
  locale,
  account,
  variant,
}: Pick<AccountSwitcherProps, "locale" | "account" | "variant">) {
  return (
    <>
      <Avatar name={account.name} locale={locale} />
      {variant === "full" && (
        <>
          <AccountFacts account={account} locale={locale} />
          <ChevronDown className={styles.chevron} aria-hidden="true" />
        </>
      )}
    </>
  );
}
