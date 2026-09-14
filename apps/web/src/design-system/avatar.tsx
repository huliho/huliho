// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Locale } from "../paraglide/runtime.js";
import styles from "./avatar.module.css";

// Two letters at most; a third says nothing the name does not.
const INITIALS_MAX = 2;

// The first letter of the first two words, in the locale's uppercase.
export function initialsOf(name: string, locale: Locale): string {
  const segmenter = new Intl.Segmenter(locale, { granularity: "grapheme" });
  return name
    .trim()
    .split(/\s+/u)
    .slice(0, INITIALS_MAX)
    .map((word) => Array.from(segmenter.segment(word), (part) => part.segment).at(0) ?? "")
    .join("")
    .toLocaleUpperCase(locale);
}

interface AvatarProps {
  name: string;
  locale: Locale;
}

// Initials only, never a fetched image. The name follows in text, so
// the circle says nothing to a screen reader.
export function Avatar({ name, locale }: AvatarProps) {
  return (
    <span className={styles.avatar} aria-hidden="true">
      {initialsOf(name, locale)}
    </span>
  );
}
