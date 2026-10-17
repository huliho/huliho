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

// Where the focus goes once a thread opens: onto its title or left in
// the list for a thread an action opened from one of its rows.
export type OpenFocus = "title" | "list";

// The mark an entry carries when the focus belongs in the list. A
// mailbox opened by a jump moves the focus into its list, where the keys
// that jumped were pressed; a thread opened by an action from a row
// leaves the focus on the row the cursor moved to. The receiver spends
// the mark once it acted, so a later visit to the entry (a reload, a
// back or a forward) moves nothing.
export function markedForList<State extends object>(state: State): State & { focusList: true } {
  return { ...state, focusList: true };
}

export function unmarkedForList(state: object): object {
  return Object.fromEntries(Object.entries(state).filter(([key]) => key !== "focusList"));
}

export function wantsListFocus(state: object): boolean {
  return "focusList" in state && state.focusList === true;
}
