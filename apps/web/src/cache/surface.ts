// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type {
  BodyDetail,
  Job,
  ListPage,
  Mailbox,
  Mutation,
  PatchMutation,
  QuotaReading,
  ThreadDetail,
  Undo,
} from "@huliho/core";

import type { CacheResult } from "./outcome";
import type { Watch } from "./watches";

// What a tab tells the worker: the accounts the session holds, when
// that list was fetched, the mailbox it watches and whether the instance
// keeps mail off the disk, renewed while the tab lives. The newest list
// wins, so a tab behind never undoes one.
export interface Lease {
  accounts: readonly string[];
  listedAt: number;
  watching: Watch | null;
  strict: boolean;
}

// The worker's surface for one tab. A call answers a result, since a
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
  body(
    accountId: string,
    emailId: string,
    options: { large: boolean },
  ): Promise<CacheResult<BodyDetail | null>>;
  mutate(accountId: string, mutation: PatchMutation): Promise<CacheResult<void>>;
  // `tab` names the tab whose lease a held destroy waits on.
  mutateMany(
    accountId: string,
    mutations: readonly Mutation[],
    tab: string,
  ): Promise<CacheResult<Undo>>;
  release(accountId: string, held: readonly number[]): Promise<CacheResult<void>>;
  drop(accountId: string, held: readonly number[]): Promise<CacheResult<void>>;
  job(accountId: string, job: Job): Promise<CacheResult<void>>;
  cancelJob(accountId: string): Promise<CacheResult<void>>;
  quota(accountId: string): Promise<CacheResult<QuotaReading | null>>;
}
