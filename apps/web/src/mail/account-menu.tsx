// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Link, useNavigate } from "@tanstack/react-router";
import { useEffect, useId } from "react";
import type { RefObject } from "react";

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
import { AccountCard, AccountFacts, triggerClass } from "./account-card";
import type { AccountSwitcherProps } from "./account-card";
import { useInboxCounts } from "./inbox-counts";
import { MonoCount } from "./mono-count";

interface AccountMenuProps extends AccountSwitcherProps {
  // The trigger stood in for this menu and was pressed meanwhile.
  openOnMount: boolean;
  // Whether the trigger that stood in held the focus when the menu landed.
  takeFocus: RefObject<boolean>;
}

// The account at the top of the sidebar and the menu behind it: every
// account of the session with its inbox's unread count and its mark,
// Settings and Sign out.
export function AccountMenu(props: AccountMenuProps) {
  const { locale, cache, accounts, account, variant, openOnMount, takeFocus } = props;
  const navigate = useNavigate();
  const signOut = useSignOut(locale);
  const counts = useInboxCounts(cache, accounts);
  // A menu open from its first render needs its trigger named, so the
  // popup has the card to sit under.
  const triggerId = useId();
  // The stand-in left with the focus when it was swapped out; the
  // trigger takes it, so it is never lost.
  useEffect(() => {
    if (takeFocus.current && document.activeElement === document.body) {
      document.getElementById(triggerId)?.focus();
    }
  }, [takeFocus, triggerId]);
  const open = (next: unknown): void => {
    if (typeof next === "string" && next !== account.id) {
      void navigate({ to: "/mail/$accountId", params: { accountId: next } });
      props.onNavigate?.();
    }
  };
  return (
    <MenuRoot defaultOpen={openOnMount} defaultTriggerId={openOnMount ? triggerId : null}>
      <MenuTrigger
        id={triggerId}
        className={triggerClass(variant)}
        aria-label={variant === "avatar" ? m.mail_account_menu({}, { locale }) : undefined}
      >
        <AccountCard locale={locale} account={account} variant={variant} />
      </MenuTrigger>
      <MenuPopup>
        <MenuRadioGroup value={account.id} onValueChange={open}>
          {accounts.map((row) => {
            const unread = counts.get(row.id) ?? 0;
            return (
              <MenuRadioItem key={row.id} value={row.id}>
                <Avatar name={row.name} locale={locale} />
                <AccountFacts account={row} locale={locale} />
                {unread > 0 && (
                  <MonoCount
                    value={unread}
                    locale={locale}
                    tone="muted"
                    label={m.header_unread({ unread }, { locale })}
                  />
                )}
              </MenuRadioItem>
            );
          })}
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
