// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// What the list lends the reading pane's toolbar: one command's action
// on the open thread, whatever the focus or a selection points at.
type RunOnOpen = (id: string) => void;

let lent: RunOnOpen | null = null;

// Lends `run` until the returned call takes it back; a later lender
// wins, and an earlier one taking back leaves the later one standing.
export function lendOpenThreadRun(run: RunOnOpen): () => void {
  lent = run;
  return () => {
    if (lent === run) {
      lent = null;
    }
  };
}

// Runs the command `id` on the open thread; nothing while no list lends.
export function runOnOpenThread(id: string): void {
  lent?.(id);
}
