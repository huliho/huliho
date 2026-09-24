// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useEffect, useRef } from "react";
import type { RefObject } from "react";

// The text of a live region. It lands in one operation after the
// region stands in the document, so a screen reader announces it. A
// sentence lands at most once per `interval`; emptying is never held.
export function useStatusText(
  ref: RefObject<HTMLElement | null>,
  text: string,
  interval = 0,
): void {
  const lastAtRef = useRef(Number.NEGATIVE_INFINITY);
  useEffect(() => {
    const region = ref.current;
    if (region === null) {
      return;
    }
    if (text !== "") {
      const now = Date.now();
      if (now - lastAtRef.current < interval) {
        return;
      }
      lastAtRef.current = now;
    }
    region.textContent = text;
  }, [ref, text, interval]);
}
