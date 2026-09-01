// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useRouter } from "@tanstack/react-router";

import { BrandMark } from "../design-system/brand-mark";
import { ErrorState } from "../design-system/error-state";
import { m } from "../paraglide/messages.js";
import { getLocale } from "../paraglide/runtime.js";
import styles from "./route-fallbacks.module.css";

export function RoutePending() {
  const locale = getLocale();
  return (
    <output className={styles.pending} aria-label={m.loading_label({}, { locale })}>
      <BrandMark stacked />
    </output>
  );
}

export function RouteError() {
  const locale = getLocale();
  const router = useRouter();
  return (
    <ErrorState
      message={m.signin_error_unavailable({}, { locale })}
      retryLabel={m.retry_action({}, { locale })}
      onRetry={() => {
        void router.invalidate();
      }}
    />
  );
}
