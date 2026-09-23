// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useRouter } from "@tanstack/react-router";

import { BrandMark } from "../design-system/brand-mark";
import { ErrorState } from "../design-system/error-state";
import { useLocale } from "../i18n/locale";
import { m } from "../paraglide/messages.js";
import styles from "./route-fallbacks.module.css";

export function RoutePending() {
  const locale = useLocale();
  return (
    <output className={styles.pending} aria-label={m.loading_label({}, { locale })}>
      <BrandMark stacked />
    </output>
  );
}

export function RouteError() {
  const locale = useLocale();
  const router = useRouter();
  return (
    <div className={styles.screen}>
      <ErrorState
        message={m.signin_error_unavailable({}, { locale })}
        retryLabel={m.retry_action({}, { locale })}
        onRetry={() => {
          void router.invalidate();
        }}
      />
    </div>
  );
}
