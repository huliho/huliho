// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Suspense, use, useRef, useState } from "react";
import type { RefObject } from "react";

import { m } from "../paraglide/messages.js";
import { chunk } from "../shell/chunk";
import { AccountCard, triggerClass } from "./account-card";
import type { AccountSwitcherProps } from "./account-card";

// The menu's code is a chunk of its own, outside the initial bundle;
// the shell fetches it on mount, so the first open finds it in.
export const prefetchAccountMenu = chunk(() => import("./account-menu"));

interface WaitingTriggerProps extends AccountSwitcherProps {
  onWant: () => void;
  // Told whether the stand-in holds the focus, read once the chunk lands.
  onHeld: (held: boolean) => void;
}

// Until the menu's code is in, the trigger is a plain button of the
// same shape; a press on it opens the menu the moment it lands.
function WaitingTrigger({ locale, account, variant, onWant, onHeld }: WaitingTriggerProps) {
  return (
    <button
      type="button"
      className={triggerClass(variant)}
      aria-haspopup="menu"
      aria-expanded={false}
      aria-label={variant === "avatar" ? m.mail_account_menu({}, { locale }) : undefined}
      onClick={onWant}
      onFocus={() => {
        onHeld(true);
      }}
      onBlur={() => {
        onHeld(false);
      }}
    >
      <AccountCard locale={locale} account={account} variant={variant} />
    </button>
  );
}

interface LoadedProps extends AccountSwitcherProps {
  openOnMount: boolean;
  takeFocus: RefObject<boolean>;
}

function LoadedAccountMenu(props: LoadedProps) {
  const { AccountMenu } = use(prefetchAccountMenu());
  return <AccountMenu {...props} />;
}

// The account at the top of the sidebar with its menu behind it; the
// menu's code comes as its own chunk, the trigger standing in for it
// while it loads.
export function AccountSwitcher(props: AccountSwitcherProps) {
  const [wanted, setWanted] = useState(false);
  const held = useRef(false);
  return (
    <Suspense
      fallback={
        <WaitingTrigger
          {...props}
          onWant={() => {
            setWanted(true);
          }}
          onHeld={(now) => {
            held.current = now;
          }}
        />
      }
    >
      <LoadedAccountMenu {...props} openOnMount={wanted} takeFocus={held} />
    </Suspense>
  );
}
