// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { applyPatch, flushPending as flushPatchLog, mayPatch } from "@huliho/core";
import type { AppliedChanges, Flushed, JmapClient, MailStore, Mutation } from "@huliho/core";

import type { CacheMessage } from "./messages";
import { attempt } from "./outcome";

// How long a change waits for the ones right behind it, so a thread
// that opens with several unread messages sends one request.
export const SET_FLUSH_MS = 300;

// What the changes ask of the coordinator: an account's client, its
// lock and the channel to the tabs.
export interface WriteHost {
  client(accountId: string): JmapClient;
  locked<Value>(accountId: string, run: (store: MailStore) => Promise<Value>): Promise<Value>;
  post(message: CacheMessage): void;
}

function moved(changes: AppliedChanges): boolean {
  return changes.mailboxes || changes.windows.length > 0 || changes.threads.length > 0;
}

// The changes a user makes to an email: each lands in the rows at once
// and waits in the patch log until the server acknowledged it.
export class Writes {
  private readonly host: WriteHost;
  private readonly timers = new Map<string, ReturnType<typeof setTimeout>>();

  constructor(host: WriteHost) {
    this.host = host;
  }

  // One change, under the account's lock: the rows take it, every tab
  // hears it and the server hears it once the flush window closed. A
  // change the account or a mailbox of the email refuses moves nothing.
  async mutate(accountId: string, mutation: Mutation): Promise<void> {
    const client = this.host.client(accountId);
    const changes = await this.host.locked(accountId, async (store) =>
      (await mayPatch(client, store, mutation)) ? applyPatch(store, accountId, mutation) : null,
    );
    if (changes === null || !moved(changes)) {
      return;
    }
    this.host.post({ kind: "changed", accountId, ...changes });
    if (!this.timers.has(accountId)) {
      this.timers.set(
        accountId,
        setTimeout(() => {
          this.timers.delete(accountId);
          void this.host.locked(accountId, (store) => this.flush(accountId, store));
        }, SET_FLUSH_MS),
      );
    }
  }

  // Sends the account's log, round after round while a next one would
  // send other rows; the caller holds the account's lock. A round that
  // fails holds nothing up: its rows wait in the log for the next flush.
  async flush(accountId: string, store: MailStore): Promise<void> {
    await attempt(() => this.round(accountId, store));
  }

  // Drops the flush an account still waits for; its rows stay logged.
  stop(accountId: string): void {
    clearTimeout(this.timers.get(accountId));
    this.timers.delete(accountId);
  }

  private async round(accountId: string, store: MailStore): Promise<void> {
    const settled = await flushPatchLog(this.host.client(accountId), store);
    this.tell(accountId, settled);
    if (settled.more) {
      await this.round(accountId, store);
    }
  }

  // Every tab hears of the rows a refusal gave back and that the server
  // refused a change.
  private tell(accountId: string, { changes, failed }: Flushed): void {
    if (moved(changes)) {
      this.host.post({ kind: "changed", accountId, ...changes });
    }
    if (failed.length > 0) {
      this.host.post({ kind: "refused" });
    }
  }
}
