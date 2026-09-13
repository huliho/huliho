// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Link } from "@tanstack/react-router";

import { useSignOut } from "../auth/use-sign-out";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import styles from "./shell-header.module.css";

// The bar above every signed-in page: the instance name, Settings and sign-out.
export function ShellHeader({ locale }: { locale: Locale }) {
  const signOut = useSignOut(locale);
  return (
    <header className={styles.topbar}>
      <span className={styles.brand}>Huliho</span>
      <nav className={styles.nav}>
        <Link to="/settings" className={styles.link}>
          {m.settings_title({}, { locale })}
        </Link>
        <button type="button" className={styles.button} onClick={signOut}>
          {m.signout_action({}, { locale })}
        </button>
      </nav>
    </header>
  );
}
