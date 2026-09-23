// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useState } from "react";

import styles from "./app.module.css";
import { useMailCache } from "./cache/use-mail-cache";
import { useLocale } from "./i18n/locale";
import { m } from "./paraglide/messages.js";
import { ShellHeader } from "./shell/shell-header";

const DEMO_MESSAGE_COUNT = 24817;

export function App() {
  const locale = useLocale();
  const [now] = useState(() => new Date());
  useMailCache(null);

  const today = new Intl.DateTimeFormat(locale, { dateStyle: "full" }).format(now);

  return (
    <main className={styles.shell}>
      <ShellHeader locale={locale} />
      <div className={styles.demo}>
        <h1 className={styles.wordmark}>Huliho</h1>
        <p className={styles.tagline}>{m.app_tagline({}, { locale })}</p>
        <p className={styles.line}>{m.demo_today({ today }, { locale })}</p>
        <p className={styles.line}>{m.demo_mailbox({ count: DEMO_MESSAGE_COUNT }, { locale })}</p>
      </div>
    </main>
  );
}
