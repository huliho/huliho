// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow } from "@huliho/core";
import { Link, useNavigate } from "@tanstack/react-router";
import { ChevronDown } from "lucide-react";

import { useSignOut } from "../auth/use-sign-out";
import { Avatar } from "../design-system/avatar";
import {
  MenuItem,
  MenuLinkItem,
  MenuPopup,
  MenuRadioGroup,
  MenuRadioItem,
  MenuRoot,
  MenuSeparator,
  MenuTrigger,
} from "../design-system/menu";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import styles from "./account-switcher.module.css";

interface AccountSwitcherProps {
  locale: Locale;
  accounts: readonly AccountRow[];
  account: AccountRow;
  // The full row names the account; the avatar alone fits the rail.
  variant: "full" | "avatar";
}

function AccountFacts({ account }: { account: AccountRow }) {
  return (
    <span className={styles.facts}>
      <span className={styles.name}>{account.name}</span>
      <span className={styles.address}>{account.address}</span>
    </span>
  );
}

// The account at the top of the sidebar and the menu behind it: every
// account of the session, Settings and Sign out.
export function AccountSwitcher({ locale, accounts, account, variant }: AccountSwitcherProps) {
  const navigate = useNavigate();
  const signOut = useSignOut(locale);
  const open = (next: unknown): void => {
    if (typeof next === "string" && next !== account.id) {
      void navigate({ to: "/mail/$accountId", params: { accountId: next } });
    }
  };
  return (
    <MenuRoot>
      <MenuTrigger
        className={variant === "full" ? styles.trigger : styles.avatarTrigger}
        aria-label={variant === "avatar" ? m.mail_account_menu({}, { locale }) : undefined}
      >
        <Avatar name={account.name} locale={locale} />
        {variant === "full" && (
          <>
            <AccountFacts account={account} />
            <ChevronDown className={styles.chevron} aria-hidden="true" />
          </>
        )}
      </MenuTrigger>
      <MenuPopup>
        <MenuRadioGroup value={account.id} onValueChange={open}>
          {accounts.map((row) => (
            <MenuRadioItem key={row.id} value={row.id}>
              <Avatar name={row.name} locale={locale} />
              <AccountFacts account={row} />
            </MenuRadioItem>
          ))}
        </MenuRadioGroup>
        <MenuSeparator />
        <MenuLinkItem render={<Link to="/settings" />}>
          {m.settings_title({}, { locale })}
        </MenuLinkItem>
        <MenuItem onClick={signOut}>{m.signout_action({}, { locale })}</MenuItem>
      </MenuPopup>
    </MenuRoot>
  );
}
