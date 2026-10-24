// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MailCache } from "@huliho/core";
import { quotaQueryOptions } from "@huliho/state";
import { useQuery } from "@tanstack/react-query";
import { Suspense, use } from "react";

import type { Locale } from "../paraglide/runtime.js";
import { prefetchMenus } from "./menus-chunk";
import type { HintedQuotaProps } from "./quota-hint";

interface QuotaFootProps {
  locale: Locale;
  cache: MailCache;
  accountId: string;
  variant: "foot" | "rail";
  className?: string | undefined;
}

function LoadedQuota(props: HintedQuotaProps) {
  const { HintedQuota } = use(prefetchMenus());
  return <HintedQuota {...props} />;
}

// The account's quota at the sidebar's foot or in the rail, absent while
// none is known. The bar rides the menus chunk, which the shell fetches on
// mount, so the start carries the query alone.
export function QuotaFoot({ locale, cache, accountId, variant, className }: QuotaFootProps) {
  const reading = useQuery(quotaQueryOptions(cache, accountId)).data;
  if (reading === undefined || reading === null) {
    return null;
  }
  return (
    <Suspense fallback={null}>
      <LoadedQuota reading={reading} locale={locale} variant={variant} className={className} />
    </Suspense>
  );
}
