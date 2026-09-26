// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useEffect, useState } from "react";

// How long a block stays while it fades out, the duration of one state flip.
const LEAVE_MS = 120;

// The value last shown, kept for the fade once it went away: `shown` is
// the value while there is one and the last one for `LEAVE_MS` after.
export function useLingering<T>(value: T | null): { shown: T | null; leaving: boolean } {
  const [held, setHeld] = useState(value);
  if (value !== null && value !== held) {
    setHeld(value);
  }
  useEffect(() => {
    if (value !== null) {
      return undefined;
    }
    const timer = setTimeout(() => {
      setHeld(null);
    }, LEAVE_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [value]);
  return { shown: value ?? held, leaving: value === null && held !== null };
}
