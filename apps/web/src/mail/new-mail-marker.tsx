// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Ref } from "react";

import { KeyCaps } from "../commands/key-caps";
import type { Chord } from "../commands/keys";
import { m } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";
import styles from "./new-mail-marker.module.css";

interface NewMailMarkerProps {
  ref: Ref<HTMLDivElement>;
  locale: Locale;
  count: number;
  // The keys that do what the button does; the name stays without them.
  keys: readonly Chord[];
  onReveal: () => void;
}

// New mail waits above the list until the user brings it in, so no row
// moves under the pointer.
export function NewMailMarker({ ref, locale, count, keys, onReveal }: NewMailMarkerProps) {
  return (
    <div ref={ref} className={styles.marker}>
      <button type="button" className={styles.button} onClick={onReveal}>
        <span>{m.list_new_mail({ count }, { locale })}</span>
        <KeyCaps keys={keys} spoken={false} />
      </button>
    </div>
  );
}
