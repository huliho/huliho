// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { useLayoutEffect, useState } from "react";
import type { RefObject } from "react";

import { DARK_QUERY } from "../../theme/use-theme";
import type { FrameStyle } from "./base-style";

// The tokens the frame takes its colors from.
const COLOR_TOKENS = {
  bg: "--hh-bg",
  surface: "--hh-surface",
  border: "--hh-border",
  text: "--hh-text",
  muted: "--hh-text-muted",
} as const;

// The document attributes that change the card's colors or type.
const APPEARANCE_ATTRIBUTES = ["data-theme", "data-density", "data-font-size", "data-line-height"];

// What the frame takes from the card around it, as the engine paints
// it. A custom property computes to its own text, so a probe wears each
// color token and is read.
export function readFrameStyle(host: HTMLElement): FrameStyle {
  const computed = getComputedStyle(host);
  const probe = host.ownerDocument.createElement("span");
  host.append(probe);
  const paint = (token: string): string => {
    probe.style.color = `var(${token})`;
    return getComputedStyle(probe).color;
  };
  const style: FrameStyle = {
    fontFamily: computed.fontFamily,
    fontSize: computed.fontSize,
    lineHeight: computed.lineHeight,
    bg: paint(COLOR_TOKENS.bg),
    surface: paint(COLOR_TOKENS.surface),
    border: paint(COLOR_TOKENS.border),
    text: paint(COLOR_TOKENS.text),
    muted: paint(COLOR_TOKENS.muted),
  };
  probe.remove();
  return style;
}

// The card's type and colors for the frame, read once the card is laid
// out and again when the theme, the system's scheme or the document's
// type attributes change; null before the first read.
export function useFrameStyle(ref: RefObject<HTMLElement | null>): FrameStyle | null {
  const [style, setStyle] = useState<FrameStyle | null>(null);
  useLayoutEffect(() => {
    const read = (): void => {
      const host = ref.current;
      if (host !== null) {
        setStyle(readFrameStyle(host));
      }
    };
    read();
    const observer = new MutationObserver(read);
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: APPEARANCE_ATTRIBUTES,
    });
    const scheme = matchMedia(DARK_QUERY);
    scheme.addEventListener("change", read);
    return () => {
      observer.disconnect();
      scheme.removeEventListener("change", read);
    };
  }, [ref]);
  return style;
}
