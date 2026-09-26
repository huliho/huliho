// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import {
  JmapClient,
  JmapError,
  applyChanges,
  firstSyncOf,
  listPage,
  queryWindow,
  readThread,
  revealNewMail,
  syncMailboxes,
} from "@huliho/core";
import type {
  AppliedChanges,
  ListPage,
  MailStore,
  Mailbox,
  StopCause,
  ThreadDetail,
} from "@huliho/core";

import type { Locks } from "./locks";
import type { CacheMessage } from "./messages";
import { attempt } from "./outcome";
import type { CacheResult } from "./outcome";

// The client asks for changes every sixty seconds until push lands.
export const CHANGES_POLL_MS = 60_000;

// While a mailbox of the account is in its first sync, the poll follows
// the batches closer, so the list and its count grow as they land.
export const FIRST_SYNC_POLL_MS = 10_000;

// A tab renews its watch this often; a hidden tab's timers still run
// once a minute, so a live tab never falls past the lease.
export const LEASE_RENEW_MS = 30_000;

// A watch older than this belongs to a tab that is gone.
export const LEASE_MS = 150_000;

// A focus poll this soon after the last one is skipped, so switching
// windows back and forth costs one request.
const FOCUS_POLL_GAP_MS = 10_000;

const EVERY_AREA = ["mailboxes", "emails", "threads", "queries"] as const;

// The mailbox one tab is looking at.
export interface Watch {
  accountId: string;
  mailboxId: string;
}

// What a tab tells the worker: the accounts the session holds, when
// that list was fetched and the mailbox it watches, renewed while the
// tab lives. The newest list wins, so a tab behind never undoes one.
export interface Lease {
  accounts: readonly string[];
  listedAt: number;
  watching: Watch | null;
}

// The worker's surface for one tab. A read answers a result, since a
// thrown error loses its fields on the way to the window.
export interface CacheApi {
  attach(lease: Lease): Promise<void>;
  focus(): Promise<void>;
  persisted(): Promise<void>;
  // Sign out: the tab names itself, so every other tab hears who did it.
  clear(by: string): Promise<void>;
  mailboxes(accountId: string): Promise<CacheResult<Mailbox[]>>;
  // A page crosses as the rows the list draws, made on this side.
  window(accountId: string, mailboxId: string, page: number): Promise<CacheResult<ListPage>>;
  thread(accountId: string, threadId: string): Promise<CacheResult<ThreadDetail | null>>;
  reveal(accountId: string, mailboxId: string): Promise<CacheResult<void>>;
}

export interface Dependencies {
  store: MailStore & { accounts(): Promise<string[]>; destroy(): Promise<void> };
  locks: Locks;
  post(message: CacheMessage): void;
}

interface Account {
  client: JmapClient;
  // The next poll; null while one runs.
  timer: ReturnType<typeof setTimeout> | null;
  lastPoll: number;
  // Whether the last poll found the account stopped on the server.
  stopped: boolean;
}

interface Watched {
  watch: Watch;
  seen: number;
}

function lockName(accountId: string): string {
  return `huliho-cache:${accountId}`;
}

// The store with every write held until the window reported the
// persistence request, so the request goes out before the first row lands.
function held(store: MailStore, gate: Promise<unknown>): MailStore {
  return {
    mailboxes: (accountId) => store.mailboxes(accountId),
    emails: (accountId, ids) => store.emails(accountId, ids),
    threads: (accountId, ids) => store.threads(accountId, ids),
    query: (accountId, mailboxId) => store.query(accountId, mailboxId),
    queries: (accountId) => store.queries(accountId),
    state: (accountId, type) => store.state(accountId, type),
    commit: async (accountId, batch) => {
      await gate;
      await store.commit(accountId, batch);
    },
  };
}

// Runs the accounts a session holds: the mailbox tree at the start, then
// the poll at its interval and on focus. Every reconciliation of one
// account runs under its lock and every change goes to every tab.
export class Coordinator {
  private readonly deps: Dependencies;
  private readonly store: MailStore;
  private readonly persisted = Promise.withResolvers<undefined>();
  private readonly accounts = new Map<string, Account>();
  private readonly watches = new Map<number, Watched>();
  // When the list in force was fetched.
  private listedAt = Number.NEGATIVE_INFINITY;
  // The last halt; only a list fetched after it starts the accounts again.
  private haltedAt = Number.NEGATIVE_INFINITY;
  // Moves on at every halt; work begun before it writes nothing.
  private generation = 0;
  private tabs = 0;

  constructor(deps: Dependencies) {
    this.deps = deps;
    this.store = held(deps.store, this.persisted.promise);
  }

  // The surface of one tab; its watch lives and dies with it.
  api(): CacheApi {
    this.tabs += 1;
    const tab = this.tabs;
    return {
      attach: (lease) => {
        if (lease.listedAt >= this.listedAt && lease.listedAt > this.haltedAt) {
          const fresh = lease.listedAt > this.listedAt;
          this.listedAt = lease.listedAt;
          this.setAccounts(lease.accounts, fresh);
        }
        this.setWatch(tab, lease.watching);
        return Promise.resolve();
      },
      focus: () => {
        this.focus();
        return Promise.resolve();
      },
      persisted: () => {
        this.persisted.resolve(undefined);
        return Promise.resolve();
      },
      clear: (by) => this.clear(by),
      mailboxes: (accountId) => attempt(() => this.mailboxes(accountId)),
      window: (accountId, mailboxId, page) =>
        attempt(async () => {
          const assembled = await this.locked(accountId, (store) =>
            queryWindow(this.client(accountId), store, mailboxId, page),
          );
          return listPage(assembled, mailboxId);
        }),
      thread: (accountId, threadId) =>
        attempt(() =>
          this.locked(accountId, (store) => readThread(this.client(accountId), store, threadId)),
        ),
      reveal: (accountId, mailboxId) => attempt(() => this.reveal(accountId, mailboxId)),
    };
  }

  // A list fresh from the server also drops what the database holds of
  // accounts outside it, an earlier session's included.
  private setAccounts(ids: readonly string[], fresh: boolean): void {
    for (const accountId of ids) {
      if (!this.accounts.has(accountId)) {
        this.accounts.set(accountId, {
          client: this.client(accountId),
          timer: null,
          lastPoll: 0,
          stopped: false,
        });
        this.schedule(accountId, 0);
      }
    }
    const stopped = [...this.accounts.keys()].filter((accountId) => !ids.includes(accountId));
    for (const accountId of stopped) {
      this.stop(accountId);
      void this.forget(accountId);
    }
    if (fresh) {
      void this.sweep([...ids, ...stopped]);
    }
  }

  // Drops every account the database holds outside the given ids.
  private async sweep(keep: readonly string[]): Promise<void> {
    const stored = await attempt(() => this.deps.store.accounts());
    if (stored.ok) {
      for (const accountId of stored.value) {
        if (!keep.includes(accountId)) {
          void this.forget(accountId);
        }
      }
    }
  }

  private setWatch(tab: number, watch: Watch | null): void {
    if (watch === null) {
      this.watches.delete(tab);
    } else {
      this.watches.set(tab, { watch, seen: Date.now() });
    }
  }

  // The mailboxes live tabs watch on an account; a watch past the lease goes.
  private watched(accountId: string): string[] {
    const live = Date.now() - LEASE_MS;
    for (const [tab, { seen }] of this.watches) {
      if (seen < live) {
        this.watches.delete(tab);
      }
    }
    const ids = [...this.watches.values()]
      .filter(({ watch }) => watch.accountId === accountId)
      .map(({ watch }) => watch.mailboxId);
    return [...new Set(ids)];
  }

  private client(accountId: string): JmapClient {
    return this.accounts.get(accountId)?.client ?? new JmapClient(accountId);
  }

  // Runs under the account's lock on a store that refuses to write once a
  // halt came after the call, so no row outlives the session that fetched
  // it; the check follows the persistence gate, since a halt may fall
  // inside that wait.
  private locked<Value>(
    accountId: string,
    run: (store: MailStore) => Promise<Value>,
  ): Promise<Value> {
    const { generation } = this;
    const store: MailStore = {
      ...this.store,
      commit: async (id, batch) => {
        await this.persisted.promise;
        if (generation !== this.generation) {
          throw new JmapError("unauthenticated");
        }
        await this.store.commit(id, batch);
      },
    };
    return this.deps.locks.request(lockName(accountId), () => run(store));
  }

  private schedule(accountId: string, delay: number): void {
    const account = this.accounts.get(accountId);
    if (account === undefined) {
      return;
    }
    if (account.timer !== null) {
      clearTimeout(account.timer);
    }
    account.timer = setTimeout(() => {
      void this.poll(accountId);
    }, delay);
  }

  private focus(): void {
    const stale = Date.now() - FOCUS_POLL_GAP_MS;
    for (const [accountId, account] of this.accounts) {
      if (account.timer !== null && account.lastPoll <= stale) {
        this.schedule(accountId, 0);
      }
    }
  }

  // One poll of one account; any failure waits for the next one, except
  // a session that ended, which stops every account and drops its rows.
  // A poll that outlived a halt leaves the next session alone.
  private async poll(accountId: string): Promise<void> {
    const account = this.accounts.get(accountId);
    if (account === undefined) {
      return;
    }
    account.timer = null;
    account.lastPoll = Date.now();
    const { generation } = this;
    const outcome = await attempt(() =>
      this.locked(accountId, (store) => this.reconcile(accountId, store)),
    );
    if (generation !== this.generation) {
      return;
    }
    if (outcome.ok) {
      this.deps.post({ kind: "changed", accountId, ...outcome.value });
    } else if (outcome.failure.code === "unauthenticated") {
      for (const stopped of this.halt()) {
        void this.forget(stopped);
      }
      return;
    }
    this.noteStop(account, accountId, outcome.ok ? null : outcome.failure.stopCause);
    this.schedule(accountId, await this.pollDelay(accountId));
  }

  // Every tab hears when the server stops an account, so the banner
  // lands with the next poll; once the account runs again they hear
  // that once.
  private noteStop(account: Account, accountId: string, stoppedCause: StopCause | null): void {
    const stopped = stoppedCause !== null;
    if (stopped || account.stopped) {
      this.deps.post({ kind: "account", accountId, stoppedCause });
    }
    account.stopped = stopped;
  }

  // The next poll comes sooner while any mailbox of the account is
  // still in its first sync.
  private async pollDelay(accountId: string): Promise<number> {
    const mailboxes = await attempt(() => this.store.mailboxes(accountId));
    const syncing = mailboxes.ok && mailboxes.value.some((row) => firstSyncOf(row) !== null);
    return syncing ? FIRST_SYNC_POLL_MS : CHANGES_POLL_MS;
  }

  // The tree first; once the account holds it, the changes since.
  private async reconcile(accountId: string, store: MailStore): Promise<AppliedChanges> {
    const client = this.client(accountId);
    if ((await store.state(accountId, "Mailbox")) === null) {
      await syncMailboxes(client, store);
      return { mailboxes: true, windows: [], threads: [] };
    }
    return applyChanges(client, store, this.watched(accountId));
  }

  private async mailboxes(accountId: string): Promise<Mailbox[]> {
    if ((await this.store.state(accountId, "Mailbox")) === null) {
      await this.locked(accountId, async (store) => {
        if ((await store.state(accountId, "Mailbox")) === null) {
          await syncMailboxes(this.client(accountId), store);
        }
      });
    }
    return this.store.mailboxes(accountId);
  }

  private async reveal(accountId: string, mailboxId: string): Promise<void> {
    await this.locked(accountId, (store) => revealNewMail(store, accountId, mailboxId));
    this.deps.post({
      kind: "changed",
      accountId,
      mailboxes: false,
      windows: [mailboxId],
      threads: [],
    });
  }

  private stop(accountId: string): void {
    const account = this.accounts.get(accountId);
    if (account?.timer !== null && account?.timer !== undefined) {
      clearTimeout(account.timer);
    }
    this.accounts.delete(accountId);
  }

  // Stops every account and marks this moment, so only a list fetched
  // after it starts them again. Answers the stopped ids.
  private halt(): string[] {
    const stopped = [...this.accounts.keys()];
    for (const accountId of stopped) {
      this.stop(accountId);
    }
    this.haltedAt = Date.now();
    this.generation += 1;
    return stopped;
  }

  // The rows of an account outside the session's list are dropped.
  private async forget(accountId: string): Promise<void> {
    const outcome = await attempt(() =>
      this.locked(accountId, (store) =>
        store.commit(accountId, {
          reset: EVERY_AREA,
          states: { Mailbox: null, Email: null, Thread: null },
        }),
      ),
    );
    if (outcome.ok) {
      this.deps.post({ kind: "changed", accountId, mailboxes: true, windows: [], threads: [] });
    }
  }

  private async clear(by: string): Promise<void> {
    this.halt();
    this.watches.clear();
    await this.deps.store.destroy();
    this.deps.post({ kind: "cleared", by });
  }
}
