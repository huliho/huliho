// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The mark an entry opened from the mailbox carries in the history
// state. On a thread it lets closing go back over the entry; on the
// card a banner's Reconnect opened it sends a pass back to the mail.
// The router's own keys stay.
export function markedFromMailbox<State extends object>(
  state: State,
): State & { fromMailbox: true } {
  return { ...state, fromMailbox: true };
}

export function openedFromMailbox(state: object): boolean {
  return "fromMailbox" in state && state.fromMailbox === true;
}
