// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// A page of the list, the unit the store serves and a query asks for;
// it equals the previews the bridge fills in one round trip.
export const WINDOW_SIZE = 100;

// How many times one poll follows hasMoreChanges before it leaves the
// rest to the next poll; the state reached is kept either way.
export const CHANGES_ROUNDS_MAX = 8;
