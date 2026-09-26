// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { FirstSync } from "@huliho/core";

import type { Locale } from "../paraglide/runtime.js";
import { FirstSyncBlock } from "./first-sync";
import { KeyHints } from "./key-hints";
import { useLingering } from "./use-lingering";
import styles from "./list-foot.module.css";

interface ListFootProps {
  locale: Locale;
  // The first sync's progress, null once every header is in.
  progress: FirstSync | null;
}

// The foot of the list: the first-sync block while the sync runs and
// through its fade, the key hints otherwise.
export function ListFoot({ locale, progress }: ListFootProps) {
  const { shown, leaving } = useLingering(progress);
  return (
    <div className={styles.foot}>
      <FirstSyncBlock locale={locale} shown={shown} leaving={leaving} />
      <KeyHints locale={locale} hidden={shown !== null} />
    </div>
  );
}
