// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useLayoutEffect, useState } from "react";
import type { RefObject } from "react";

export const ROW_HEIGHT_TOKEN = "--hhx-row-height";
export const TOOLBAR_HEIGHT_TOKEN = "--hhx-toolbar-height";
// The key-hint strip at the foot of the list.
export const FOOT_HEIGHT_TOKEN = "--hhx-foot-height";
// The comfortable values, until the tokens are read from the stylesheet.
export const ROW_HEIGHT_FALLBACK_PX = 52;
export const TOOLBAR_HEIGHT_FALLBACK_PX = 44;
export const FOOT_HEIGHT_FALLBACK_PX = 36;

// A length token as the density resolves it on the element, in CSS
// pixels: read on mount and again when the density on the document
// changes.
export function useTokenPx(
  ref: RefObject<HTMLElement | null>,
  token: string,
  fallback: number,
): number {
  const [value, setValue] = useState(fallback);
  useLayoutEffect(() => {
    const read = (): void => {
      const element = ref.current;
      if (element === null) {
        return;
      }
      const measured = Number.parseFloat(getComputedStyle(element).getPropertyValue(token));
      if (Number.isFinite(measured) && measured > 0) {
        setValue(measured);
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
  }, [ref, token]);
  return value;
}

// The density's row height.
export function useRowHeight(ref: RefObject<HTMLElement | null>): number {
  return useTokenPx(ref, ROW_HEIGHT_TOKEN, ROW_HEIGHT_FALLBACK_PX);
}
