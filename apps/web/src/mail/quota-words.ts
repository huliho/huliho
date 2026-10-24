// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { QuotaReading } from "@huliho/core";

import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";

// The units a storage quota reads in, each 1000 of the one before, as SI
// and the unit names `Intl` writes define them.
const UNITS = ["kilobyte", "megabyte", "gigabyte", "terabyte", "petabyte"] as const;
const UNIT_STEP = 1000;
// A figure below this keeps one decimal and one from it none: 4.2 of 10 GB.
const DECIMALS_BELOW = 10;

export interface QuotaWords {
  // Over the bar: "4.2 of 10 GB".
  words: string;
  // The sentence that names the meter and shows in its hint.
  detail: string;
  // "Almost full" once the quota reaches its warn limit.
  warning: string | null;
}

interface Scale {
  unit: (typeof UNITS)[number];
  size: number;
}

// The largest unit the limit holds at least one of, the kilobyte at the least.
function scaleOf(limit: number): Scale {
  let scale: Scale = { unit: "kilobyte", size: UNIT_STEP };
  for (const [index, unit] of UNITS.entries()) {
    const size = UNIT_STEP ** (index + 1);
    if (limit >= size) {
      scale = { unit, size };
    }
  }
  return scale;
}

function figure(value: number, locale: Locale, unit?: Scale["unit"]): string {
  const digits = value < DECIMALS_BELOW ? 1 : 0;
  const style: Intl.NumberFormatOptions = unit === undefined ? {} : { style: "unit", unit };
  return new Intl.NumberFormat(locale, { maximumFractionDigits: digits, ...style }).format(value);
}

// Both figures read in the unit of the limit; a count quota reads in messages.
export function quotaWords(reading: QuotaReading, locale: Locale): QuotaWords {
  const warning = reading.warn ? m.quota_almost_full({}, { locale }) : null;
  if (reading.unit === "count") {
    const counts = { used: reading.used, limit: reading.limit };
    return {
      words: m.quota_messages(counts, { locale }),
      detail: m.quota_detail_messages(counts, { locale }),
      warning,
    };
  }
  const { unit, size } = scaleOf(reading.limit);
  const used = reading.used / size;
  const limit = figure(reading.limit / size, locale, unit);
  return {
    words: m.quota_words({ used: figure(used, locale), limit }, { locale }),
    detail: m.quota_detail({ used: figure(used, locale, unit), limit }, { locale }),
    warning,
  };
}
