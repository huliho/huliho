// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Suspense, use, useId, useRef, useState } from "react";
import type { RefObject } from "react";

import type { Chord } from "../commands/keys";
import { useCommand } from "../commands/use-command";
import { m } from "../paraglide/messages.js";
import { chunk } from "../shell/chunk";
import { AccountCard, triggerClass } from "./account-card";
import type { AccountSwitcherProps } from "./account-card";

// The menu's code is a chunk of its own, outside the initial bundle;
// the shell fetches it on mount, so the first open finds it in.
export const prefetchAccountMenu = chunk(() => import("./account-menu"));

// The combination that opens the menu from anywhere on the screen.
const SWITCH_KEYS: readonly Chord[] = [{ key: "l", mod: true, shift: true }];

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
  triggerId: string;
  takeFocus: RefObject<boolean>;
}

function LoadedAccountMenu(props: LoadedProps) {
  const { AccountMenu } = use(prefetchAccountMenu());
  return <AccountMenu {...props} />;
}

// The account at the top of the sidebar with its menu behind it; the
// menu's code comes as its own chunk, the trigger standing in for it
// while it loads. The switch command presses the trigger, so the menu
// opens as it does from a press; while the stand-in is up, the press
// is kept for the landing.
export function AccountSwitcher(props: AccountSwitcherProps) {
  const [wanted, setWanted] = useState(false);
  const held = useRef(false);
  const triggerId = useId();
  useCommand({
    id: "account.switch",
    label: m.command_switch_account({}, { locale: props.locale }),
    group: "app",
    keys: SWITCH_KEYS,
    run: () => {
      const trigger = document.getElementById(triggerId);
      if (trigger === null) {
        setWanted(true);
      } else {
        trigger.click();
      }
    },
  });
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
      <LoadedAccountMenu {...props} openOnMount={wanted} triggerId={triggerId} takeFocus={held} />
    </Suspense>
  );
}
