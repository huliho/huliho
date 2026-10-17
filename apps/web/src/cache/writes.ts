// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import {
  dropHeld,
  flushPending as flushPatchLog,
  mayPatch,
  mutateMany,
  releaseHeld,
  releaseLapsed,
  verbOf,
} from "@huliho/core";
import type {
  AppliedChanges,
  Flushed,
  JmapClient,
  MailStore,
  Mutation,
  PatchMutation,
  PendingRow,
  Undo,
} from "@huliho/core";

import type { CacheMessage } from "./messages";
import { attempt } from "./outcome";

// How long a change waits for the ones right behind it, so a thread
// that opens with several unread messages sends one request.
export const SET_FLUSH_MS = 300;

// What the changes ask of the coordinator: an account's client, its
// lock, the channel to the tabs and the tabs that hold a lease now.
export interface WriteHost {
  client(accountId: string): JmapClient;
  locked<Value>(accountId: string, run: (store: MailStore) => Promise<Value>): Promise<Value>;
  post(message: CacheMessage): void;
  leases(): Promise<ReadonlySet<string>>;
}

// Whether some changes touched anything a tab reads.
export function moved(changes: AppliedChanges): boolean {
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

  async mutate(accountId: string, mutation: PatchMutation): Promise<void> {
    await this.mutateMany(accountId, [mutation]);
  }

  // Several changes under one hold of the account's lock: the rows take
  // them, every tab hears it and the server hears them once the flush
  // window closed. A change the account or a mailbox of the email
  // refuses moves nothing; a destroy waits held on the lease of `tab`.
  async mutateMany(accountId: string, mutations: readonly Mutation[], tab?: string): Promise<Undo> {
    const client = this.host.client(accountId);
    const logged = await this.host.locked(accountId, async (store) => {
      const allowed = await Promise.all(
        mutations.map((mutation) => mayPatch(client, store, mutation)),
      );
      return mutateMany(
        store,
        accountId,
        mutations.filter((_mutation, index) => allowed.at(index) === true),
        tab,
      );
    });
    if (moved(logged.changes)) {
      this.host.post({ kind: "changed", accountId, ...logged.changes });
      this.soon(accountId);
    }
    return { inverses: logged.inverses, held: logged.held, threads: logged.changes.threads };
  }

  // The window's toast ended: the held destroys go out with the next flush.
  async release(accountId: string, held: readonly number[]): Promise<void> {
    const released = await this.host.locked(accountId, (store) =>
      releaseHeld(store, accountId, held),
    );
    if (released) {
      this.soon(accountId);
    }
  }

  // Undo of a delete forever: the emails show again where they stood.
  async drop(accountId: string, held: readonly number[]): Promise<void> {
    const changes = await this.host.locked(accountId, (store) => dropHeld(store, accountId, held));
    if (moved(changes)) {
      this.host.post({ kind: "changed", accountId, ...changes });
    }
  }

  // Sends the account's log, round after round while a next one would
  // send other rows; the caller holds the account's lock. A held destroy
  // whose tab holds no lease is released first. A round that fails
  // holds nothing up: its rows wait in the log for the next flush.
  async flush(accountId: string, store: MailStore): Promise<void> {
    await attempt(async () => releaseLapsed(store, accountId, await this.host.leases()));
    await attempt(() => this.round(accountId, store));
  }

  // Drops the flush an account still waits for; its rows stay logged.
  stop(accountId: string): void {
    clearTimeout(this.timers.get(accountId));
    this.timers.delete(accountId);
  }

  // The first change of an account starts the flush window; later ones
  // ride along without moving its end.
  private soon(accountId: string): void {
    if (this.timers.has(accountId)) {
      return;
    }
    this.timers.set(
      accountId,
      setTimeout(() => {
        this.timers.delete(accountId);
        void this.host.locked(accountId, (store) => this.flush(accountId, store));
      }, SET_FLUSH_MS),
    );
  }

  private async round(accountId: string, store: MailStore): Promise<void> {
    const logged = await store.pending(accountId);
    const settled = await flushPatchLog(this.host.client(accountId), store);
    await this.tell(accountId, store, settled, logged);
    if (settled.more) {
      await this.round(accountId, store);
    }
  }

  // Every tab hears of the rows a refusal gave back and of the emails
  // the server refused, with what their changes did.
  private async tell(
    accountId: string,
    store: MailStore,
    { changes, failed }: Flushed,
    logged: readonly PendingRow[],
  ): Promise<void> {
    if (moved(changes)) {
      this.host.post({ kind: "changed", accountId, ...changes });
    }
    if (failed.length === 0) {
      return;
    }
    const mailboxes = await store.mailboxes(accountId);
    const verbs = logged
      .filter((row) => failed.includes(row.id))
      .map((row) => verbOf(row, mailboxes));
    this.host.post({ kind: "refused", accountId, emailIds: failed, verbs: [...new Set(verbs)] });
  }
}
