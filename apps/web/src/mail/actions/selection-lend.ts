// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// What the list lends the surfaces beside it: the pane header's Select
// opens the mode, the thread's Escape closes a standing one first.
export interface SelectionLend {
  start: () => void;
  // Closes the mode; false when none stood.
  close: () => boolean;
}

let lent: SelectionLend | null = null;

// Lends `selection` until the returned call takes it back; a later
// lender wins, and an earlier one taking back leaves it standing.
export function lendSelection(selection: SelectionLend): () => void {
  lent = selection;
  return () => {
    if (lent === selection) {
      lent = null;
    }
  };
}

export function startSelection(): void {
  lent?.start();
}

// Closes a standing mode; false while none stands or no list lends.
export function closeSelection(): boolean {
  return lent?.close() ?? false;
}
