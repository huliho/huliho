// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// A page of the list, the unit the store serves and a query asks for;
// it equals the previews the bridge fills in one round trip.
export const WINDOW_SIZE = 100;

// The previews a bridge fills per Email/get, so a thread's members are
// asked for in gets of at most this many.
export const PREVIEW_BATCH = 100;

// How many times one poll follows hasMoreChanges before it leaves the
// rest to the next poll; the state reached is kept either way.
export const CHANGES_ROUNDS_MAX = 8;

// The bodies a store keeps; past it the ones fetched longest ago leave.
export const BODY_CACHE_ROWS = 500;

// The weight of body values a store keeps, 64 MiB; past it the same
// bodies leave.
export const BODY_CACHE_BYTES = 64 * 1024 * 1024;

// The searches an account keeps in memory; a new one past it drops the
// one that ran first, so going back to a recent search costs nothing.
export const SEARCH_ROWS = 4;

// The senders a suggestion list shows at most, as many as fit above the
// keyboard on a phone.
export const SENDER_SUGGESTIONS = 5;
