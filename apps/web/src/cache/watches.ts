// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// A watch older than this belongs to a tab that is gone.
export const LEASE_MS = 150_000;

// The mailbox one tab is looking at.
export interface Watch {
  accountId: string;
  mailboxId: string;
}

interface Watched {
  watch: Watch;
  seen: number;
}

// The mailbox each tab looks at, by the tab's number; a watch the tab
// stops renewing lapses with its lease.
export class Watches {
  private readonly held = new Map<number, Watched>();

  set(tab: number, watch: Watch | null): void {
    if (watch === null) {
      this.held.delete(tab);
    } else {
      this.held.set(tab, { watch, seen: Date.now() });
    }
  }

  // The mailboxes live tabs watch on an account; a watch past the lease goes.
  of(accountId: string): string[] {
    const live = Date.now() - LEASE_MS;
    for (const [tab, { seen }] of this.held) {
      if (seen < live) {
        this.held.delete(tab);
      }
    }
    const ids = [...this.held.values()]
      .filter(({ watch }) => watch.accountId === accountId)
      .map(({ watch }) => watch.mailboxId);
    return [...new Set(ids)];
  }

  clear(): void {
    this.held.clear();
  }
}
