// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// State-changing calls carry this header; the server refuses them without it.
export const CSRF_HEADERS = { "x-requested-with": "huliho" } as const;
