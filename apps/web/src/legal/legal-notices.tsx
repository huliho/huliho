// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { cx } from "../design-system/cx";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import styles from "./legal-notices.module.css";

// Both lines render verbatim in every locale, per the NOTICE terms.
const ATTRIBUTION = "Huliho, by Eric Kochen";
const COPYRIGHT_LINE = "Copyright (C) 2026 Eric Kochen";

const SOURCE_URL = "https://github.com/huliho/huliho";

// The server ships the AGPL text on this route.
const LICENSE_PATH = "/license";

interface LegalNoticesProps {
  locale: Locale;
  align?: "center" | "start";
}

export function LegalNotices({ locale, align = "center" }: LegalNoticesProps) {
  return (
    <div className={cx(styles.notices, align === "start" ? styles.start : undefined)}>
      <p className={styles.names}>
        <span>{ATTRIBUTION}</span>
        <span aria-hidden="true" className={styles.separator}>
          ·
        </span>
        <span>{COPYRIGHT_LINE}</span>
      </p>
      <p className={styles.line}>{m.legal_no_warranty({}, { locale })}</p>
      <p className={styles.line}>{m.legal_may_convey({}, { locale })}</p>
      <p className={styles.links}>
        <a href={LICENSE_PATH} target="_blank" rel="noopener noreferrer">
          {m.legal_license_link({}, { locale })}
        </a>
        <a href={SOURCE_URL} target="_blank" rel="noopener noreferrer">
          {m.legal_source_link({}, { locale })}
        </a>
      </p>
    </div>
  );
}
