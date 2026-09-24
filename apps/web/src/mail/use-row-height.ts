// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useLayoutEffect, useState } from "react";
import type { RefObject } from "react";

// The comfortable row height, until the token is read from the stylesheet.
const ROW_HEIGHT_FALLBACK_PX = 52;
const ROW_HEIGHT_TOKEN = "--hhx-row-height";

// The density's row height, read from the stylesheet on mount and again
// when the density on the document changes.
export function useRowHeight(listRef: RefObject<HTMLElement | null>): number {
  const [height, setHeight] = useState(ROW_HEIGHT_FALLBACK_PX);
  useLayoutEffect(() => {
    const read = (): void => {
      const element = listRef.current;
      if (element === null) {
        return;
      }
      const value = Number.parseFloat(getComputedStyle(element).getPropertyValue(ROW_HEIGHT_TOKEN));
      if (Number.isFinite(value) && value > 0) {
        setHeight(value);
      }
    };
    read();
    const observer = new MutationObserver(read);
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-density"],
    });
    return () => {
      observer.disconnect();
    };
  }, [listRef]);
  return height;
}
