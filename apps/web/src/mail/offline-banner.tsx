// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useRef } from "react";

import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import { useStatusText } from "./status-text";
import styles from "./offline-banner.module.css";

interface OfflineBannerProps {
  locale: Locale;
  online: boolean;
}

// The strip over the list while the device is offline: the rows stay
// readable from the cache. The status region is always in the DOM and
// the sentence lands in it once the strip renders.
export function OfflineBanner({ locale, online }: OfflineBannerProps) {
  const sentenceRef = useRef<HTMLParagraphElement>(null);
  useStatusText(sentenceRef, online ? "" : m.list_offline({}, { locale }));
  return (
    <div className={styles.banner} data-idle={online || undefined}>
      <span className={styles.dot} aria-hidden="true" />
      <p ref={sentenceRef} role="status" className={styles.sentence} />
    </div>
  );
}
