// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { QuotaReading } from "@huliho/core";

import { QuotaBar } from "../design-system/quota-bar";
import { Tooltip } from "../design-system/tooltip";
import type { Locale } from "../paraglide/runtime.js";
import { quotaWords } from "./quota-words";

export interface HintedQuotaProps {
  reading: QuotaReading;
  locale: Locale;
  variant: "foot" | "rail";
  className?: string | undefined;
}

// The quota's bar, a tab stop whose hint names the figures on hover and on
// focus; the bar sits at the viewport's foot, so the hint opens over it.
export function HintedQuota({ reading, locale, variant, className }: HintedQuotaProps) {
  const words = quotaWords(reading, locale);
  return (
    <Tooltip label={words.detail} keys={[]} side="top">
      <QuotaBar
        {...words}
        used={reading.used}
        limit={reading.limit}
        locale={locale}
        variant={variant}
        tabIndex={0}
        className={className}
      />
    </Tooltip>
  );
}
