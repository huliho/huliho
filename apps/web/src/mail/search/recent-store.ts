// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { forgetStored } from "../forget-stored";

// One key per account holds its recent searches on this device.
export const RECENT_KEY_PREFIX = "huliho-recent-searches:";

// The recents of an instance that keeps mail off the disk, per account.
export const keptRecents = new Map<string, string[]>();

// A session that ends in this browser takes every account's recents with it.
export function forgetRecentSearches(): void {
  keptRecents.clear();
  forgetStored(RECENT_KEY_PREFIX);
}
