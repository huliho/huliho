// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useSyncExternalStore } from "react";

import { startOfDay } from "./row-time";

function today(): number {
  return startOfDay(new Date());
}

// The next local midnight from the calendar, so a daylight saving
// change never lands it an hour off.
function nextMidnight(now: Date): number {
  return new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1).getTime();
}

// Midnight moves the day and the timer is set again for the next one; a
// tab that slept through one hears it when it comes back into view.
function subscribe(onChange: () => void): () => void {
  let timer: ReturnType<typeof setTimeout>;
  const arm = (): void => {
    timer = setTimeout(
      () => {
        onChange();
        arm();
      },
      nextMidnight(new Date()) - Date.now(),
    );
  };
  arm();
  document.addEventListener("visibilitychange", onChange);
  return () => {
    clearTimeout(timer);
    document.removeEventListener("visibilitychange", onChange);
  };
}

// The start of the local day, so a list's times read against it.
export function useToday(): number {
  return useSyncExternalStore(subscribe, today, today);
}
