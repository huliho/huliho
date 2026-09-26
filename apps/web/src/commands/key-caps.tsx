// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { Kbd } from "../design-system/kbd";
import { keysText } from "./keys";
import type { Chord } from "./keys";

interface KeyCapsProps {
  keys: readonly Chord[];
  // Whether a screen reader reads the keys; a hint beside a spoken label stays silent.
  spoken?: boolean;
}

// The keys of a command as one cap; nothing for a command without keys.
export function KeyCaps({ keys, spoken = true }: KeyCapsProps) {
  if (keys.length === 0) {
    return null;
  }
  return <Kbd spoken={spoken}>{keysText(keys)}</Kbd>;
}
