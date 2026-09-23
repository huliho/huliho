// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { isPreferenceLocale } from "@huliho/core";
import { preferencesQueryOptions } from "@huliho/state";
import { useQuery } from "@tanstack/react-query";

import { ErrorState } from "../../design-system/error-state";
import { Skeleton } from "../../design-system/skeleton";
import { switchLocale, useLocale } from "../../i18n/locale";
import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";
import { AppearanceForm } from "./appearance-form";
import { usePreference } from "./use-preference";
import styles from "./appearance.module.css";

// Theme, density, reading pane and language: the four cards the page holds.
const SETTING_COUNT = 4;

// Still cards in the shape of the settings while the choices load.
// Spans only: an output element takes phrasing content.
export function AppearanceSkeleton({ locale }: { locale: Locale }) {
  return (
    <output className={styles.skeleton} aria-label={m.loading_label({}, { locale })}>
      {Array.from({ length: SETTING_COUNT }, (_, index) => (
        <span key={index} className={styles.skeletonCard}>
          <Skeleton className={styles.skeletonTitle} />
          <Skeleton className={styles.skeletonControl} />
        </span>
      ))}
    </output>
  );
}

export function AppearancePage() {
  const locale = useLocale();
  const query = useQuery(preferencesQueryOptions);
  const change = usePreference(locale);
  // The screen switches at once; the server hears about the words it stores.
  const pickLocale = (next: Locale): void => {
    switchLocale(next);
    if (isPreferenceLocale(next)) {
      change({ key: "locale", value: next });
    }
  };
  if (query.isPending) {
    return <AppearanceSkeleton locale={locale} />;
  }
  if (query.isError) {
    return (
      <ErrorState
        message={m.appearance_error({}, { locale })}
        retryLabel={m.retry_action({}, { locale })}
        onRetry={() => {
          void query.refetch();
        }}
      />
    );
  }
  return (
    <AppearanceForm
      locale={locale}
      preferences={query.data}
      onChange={change}
      onSwitchLocale={pickLocale}
    />
  );
}
