// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { readSenders, searchWindow } from "@huliho/core";
import type { JmapClient, ListPage, MailStore, Sender } from "@huliho/core";

import { attempt } from "./outcome";
import type { CacheResult } from "./outcome";
import type { SearchTicket } from "./surface";

// What the searches ask of the coordinator: an account's client, its
// lock and the store a read goes to.
export interface SearchHost {
  client(accountId: string): JmapClient;
  locked<Value>(accountId: string, run: (store: MailStore) => Promise<Value>): Promise<Value>;
  store: MailStore;
}

// The searches the tabs ask for. A search runs under the account's lock,
// so no poll lands rows under it; a signal cannot cross to the worker, so
// each call keeps a controller its ticket can give up.
export class Searches {
  private readonly host: SearchHost;
  private readonly calls = new Map<string, AbortController>();

  constructor(host: SearchHost) {
    this.host = host;
  }

  async search(
    accountId: string,
    key: string,
    page: number,
    { locale, again, ticket }: SearchTicket,
  ): Promise<CacheResult<ListPage>> {
    const controller = new AbortController();
    this.calls.set(ticket, controller);
    try {
      return await attempt(() =>
        this.host.locked(accountId, (store) => {
          controller.signal.throwIfAborted();
          return searchWindow(this.host.client(accountId), store, {
            key,
            page,
            locale,
            again,
            signal: controller.signal,
          });
        }),
      );
    } finally {
      this.calls.delete(ticket);
    }
  }

  // A call already answered is left alone.
  cancel(ticket: string): void {
    this.calls.get(ticket)?.abort();
  }

  // The senders read the store alone, so they answer while a search runs.
  senders(accountId: string, prefix: string): Promise<CacheResult<Sender[]>> {
    return attempt(() => readSenders(this.host.store, accountId, prefix));
  }
}
