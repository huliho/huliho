// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The mark a row opened from its list leaves on the history entry, so
// closing the thread can go back over it; the router's own keys stay.
export function markedFromMailbox<State extends object>(
  state: State,
): State & { fromMailbox: true } {
  return { ...state, fromMailbox: true };
}

export function openedFromMailbox(state: object): boolean {
  return "fromMailbox" in state && state.fromMailbox === true;
}
