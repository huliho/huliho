// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useEffect, useState } from "react";

const COUNTDOWN_TICK_MS = 1_000;

export interface RetryCountdown {
  // Seconds until the limiter lets the next attempt through; null once it does.
  retryRemaining: number | null;
  start: (seconds: number) => void;
}

function countDown(remaining: number | null): number | null {
  return remaining !== null && remaining > 1 ? remaining - 1 : null;
}

// The seconds a rate-limited refusal announced, ticking down to null.
export function useRetryCountdown(): RetryCountdown {
  const [retryRemaining, setRetryRemaining] = useState<number | null>(null);
  const active = retryRemaining !== null;
  useEffect(() => {
    if (!active) {
      return undefined;
    }
    const timer = setInterval(() => {
      setRetryRemaining(countDown);
    }, COUNTDOWN_TICK_MS);
    return () => {
      clearInterval(timer);
    };
  }, [active]);
  return { retryRemaining, start: setRetryRemaining };
}
