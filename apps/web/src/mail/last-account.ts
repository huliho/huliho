// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { AccountRow } from "@huliho/core";

const LAST_ACCOUNT_KEY = "huliho-last-account";

export function rememberLastAccount(id: string): void {
  localStorage.setItem(LAST_ACCOUNT_KEY, id);
}

// A session that ends in this browser takes the memory with it, so the
// next sign-in starts at the oldest account.
export function forgetLastAccount(): void {
  localStorage.removeItem(LAST_ACCOUNT_KEY);
}

// Where the root lands: the account this device opened last while the
// session still holds it, else the oldest account; null without any.
export function landingAccount(accounts: readonly AccountRow[]): string | null {
  const remembered = localStorage.getItem(LAST_ACCOUNT_KEY);
  if (remembered !== null && accounts.some((row) => row.id === remembered)) {
    return remembered;
  }
  const oldest = accounts.toSorted(
    (first, second) => first.createdAt - second.createdAt || first.id.localeCompare(second.id),
  );
  return oldest.at(0)?.id ?? null;
}
