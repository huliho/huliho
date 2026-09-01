// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// A locale is listed in its own language, so every reader finds theirs.
export function localeEndonym(locale: string): string {
  const names = new Intl.DisplayNames([locale], { type: "language" });
  return names.of(locale) ?? locale;
}
