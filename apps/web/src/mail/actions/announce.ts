// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// A sentence for the list's status region, so a reader whose focus
// moved hears it too. The region empties first, so the same sentence
// twice is said twice.
export function announce(region: HTMLElement | null, text: string): void {
  if (region === null) {
    return;
  }
  region.textContent = "";
  requestAnimationFrame(() => {
    region.textContent = text;
  });
}
