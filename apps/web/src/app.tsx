// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { localeEndonym, PSEUDO_LOCALE } from "@huliho/i18n";
import { Link } from "@tanstack/react-router";
import { useEffect, useState } from "react";

import styles from "./app.module.css";
import { useSignOut } from "./auth/use-sign-out";
import { m } from "./paraglide/messages.js";
import { getLocale, isLocale, locales, setLocale } from "./paraglide/runtime.js";
import type { Locale } from "./paraglide/runtime.js";

const DEMO_MESSAGE_COUNT = 24817;

function ShellHeader({ locale }: { locale: Locale }) {
  const signOut = useSignOut(locale);
  return (
    <header className={styles.topbar}>
      <span className={styles.topbarBrand}>Huliho</span>
      <nav className={styles.topbarNav}>
        <Link to="/settings" className={styles.topbarLink}>
          {m.settings_title({}, { locale })}
        </Link>
        <button type="button" className={styles.topbarButton} onClick={signOut}>
          {m.signout_action({}, { locale })}
        </button>
      </nav>
    </header>
  );
}

// The pseudo locale is a development aid, so only development builds list it.
function listedLocales(current: Locale): Locale[] {
  const listed = import.meta.env.DEV
    ? [...locales]
    : locales.filter((locale) => locale !== PSEUDO_LOCALE);
  return listed.includes(current) ? listed : [...listed, current];
}

export function App() {
  const [locale, setActiveLocale] = useState<Locale>(getLocale());
  const [now] = useState(() => new Date());

  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);

  const today = new Intl.DateTimeFormat(locale, { dateStyle: "full" }).format(now);

  return (
    <main className={styles.shell}>
      <ShellHeader locale={locale} />
      <div className={styles.demo}>
        <h1 className={styles.wordmark}>Huliho</h1>
        <p className={styles.tagline}>{m.app_tagline({}, { locale })}</p>
        <p className={styles.line}>{m.demo_today({ today }, { locale })}</p>
        <p className={styles.line}>{m.demo_mailbox({ count: DEMO_MESSAGE_COUNT }, { locale })}</p>
        <label className={styles.locale}>
          {m.locale_label({}, { locale })}
          <select
            className={styles.select}
            value={locale}
            onChange={(event) => {
              const next = event.target.value;
              if (isLocale(next)) {
                void setLocale(next, { reload: false });
                setActiveLocale(next);
              }
            }}
          >
            {listedLocales(locale).map((listed) => (
              <option key={listed} value={listed}>
                {localeEndonym(listed)}
              </option>
            ))}
          </select>
        </label>
      </div>
    </main>
  );
}
