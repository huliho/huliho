// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { JmapError, MethodFailure } from "@huliho/core";
import type { ListPage, MailCache, Mutation, SearchCall, Undo } from "@huliho/core";
import { MAIL_KEY_WORDS, queryKeys, sessionQueryOptions } from "@huliho/state";
import type { QueryClient } from "@tanstack/react-query";
import { wrap } from "comlink";
import type { Remote } from "comlink";

import { LEASE_RENEW_MS } from "./coordinator";
import type { CacheApi, Lease } from "./coordinator";
import { LEASE_LOCK } from "./locks";
import { CACHE_CHANNEL, readCacheMessage } from "./messages";
import type { CacheMessage } from "./messages";
import type { CacheResult } from "./outcome";

const WORKER_NAME = "huliho-cache";

// This tab's name on the channel, so it can tell its own sign-out from
// another tab's.
export const TAB = crypto.randomUUID();

// A worker that never answers must not hold the sign-out; the database
// still goes when the worker gets to it.
export const CLEAR_WAIT_MS = 5_000;

let remote: Remote<CacheApi> | null = null;
let persistenceRequested = false;

// The worker starts on first use: one per origin where the browser has
// shared workers, one per tab otherwise.
function worker(): Remote<CacheApi> {
  if (remote !== null) {
    return remote;
  }
  const endpoint =
    "SharedWorker" in globalThis
      ? new SharedWorker(new URL("./worker.ts", import.meta.url), {
          type: "module",
          name: WORKER_NAME,
        }).port
      : new Worker(new URL("./worker.ts", import.meta.url), { type: "module", name: WORKER_NAME });
  remote = wrap<CacheApi>(endpoint);
  return remote;
}

function passed(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

// A failure crosses the boundary as data; here it becomes the error the
// query hooks read, its cause, limit and method error intact.
async function unwrap<Value>(answer: Promise<CacheResult<Value>>): Promise<Value> {
  const result = await answer;
  if (result.ok) {
    return result.value;
  }
  const { failure } = result;
  if (failure.method !== null) {
    throw new MethodFailure(failure.method.type, failure.method.call);
  }
  throw new JmapError(failure.code, {
    ...(failure.stopCause === null ? {} : { stopCause: failure.stopCause }),
    ...(failure.limit === null ? {} : { limit: failure.limit }),
  });
}

// The lock that tells every worker this tab lives, held while a destroy
// it asked waits held; one hold per answer that held one.
interface LeaseLock {
  granted: Promise<void>;
  release: () => void;
}

let leaseLock: LeaseLock | null = null;
let leaseHolds = 0;

function holdLease(): Promise<void> {
  leaseHolds += 1;
  if (leaseLock === null) {
    const granted = Promise.withResolvers<undefined>();
    const released = Promise.withResolvers<undefined>();
    void navigator.locks.request(`${LEASE_LOCK}${TAB}`, () => {
      granted.resolve(undefined);
      return released.promise;
    });
    leaseLock = {
      granted: granted.promise,
      release: () => {
        released.resolve(undefined);
      },
    };
  }
  return leaseLock.granted;
}

function letLease(): void {
  leaseHolds = Math.max(0, leaseHolds - 1);
  if (leaseHolds === 0 && leaseLock !== null) {
    leaseLock.release();
    leaseLock = null;
  }
}

// A change that destroys holds the lease before the worker logs it, so
// no worker releases the destroy while this tab can still take it back.
async function mutateMany(accountId: string, mutations: readonly Mutation[]): Promise<Undo> {
  const destroying = mutations.some((mutation) => "destroy" in mutation);
  if (!destroying) {
    return unwrap(worker().mutateMany(accountId, mutations, TAB));
  }
  await holdLease();
  try {
    const undo = await unwrap(worker().mutateMany(accountId, mutations, TAB));
    if (undo.held.length === 0) {
      letLease();
    }
    return undo;
  } catch (error) {
    letLease();
    throw error;
  }
}

// What ends a hold: the destroys go out or come back.
async function endHold(settle: Promise<CacheResult<void>>): Promise<void> {
  try {
    await unwrap(settle);
  } finally {
    letLease();
  }
}

// A signal cannot cross to the worker, so a search goes with a ticket: a
// signal that aborts gives the call up there and rejects here at once.
async function search(
  accountId: string,
  key: string,
  page: number,
  { locale, again = false, signal }: SearchCall,
): Promise<ListPage> {
  signal?.throwIfAborted();
  const ticket = crypto.randomUUID();
  const answer = unwrap(worker().search(accountId, key, page, { locale, again, ticket }));
  if (signal === undefined) {
    return answer;
  }
  const given = Promise.withResolvers<never>();
  const abort = (): void => {
    void worker().cancelSearch(ticket);
    given.reject(signal.reason);
  };
  signal.addEventListener("abort", abort, { once: true });
  try {
    return await Promise.race([answer, given.promise]);
  } finally {
    signal.removeEventListener("abort", abort);
  }
}

// The cache as the query hooks read it, served by the worker. Each held
// answer of `mutateMany` ends with one `release` or one `drop`.
export const mailCache: MailCache = {
  mailboxes: (accountId) => unwrap(worker().mailboxes(accountId)),
  window: (accountId, mailboxId, page) => unwrap(worker().window(accountId, mailboxId, page)),
  search,
  senders: (accountId, prefix) => unwrap(worker().senders(accountId, prefix)),
  thread: (accountId, threadId) => unwrap(worker().thread(accountId, threadId)),
  reveal: (accountId, mailboxId) => unwrap(worker().reveal(accountId, mailboxId)),
  body: (accountId, emailId, options) => unwrap(worker().body(accountId, emailId, options)),
  mutate: (accountId, mutation) => unwrap(worker().mutate(accountId, mutation)),
  mutateMany,
  release: (accountId, held) => endHold(worker().release(accountId, held)),
  drop: (accountId, held) => endHold(worker().drop(accountId, held)),
  job: (accountId, job) => unwrap(worker().job(accountId, job)),
  cancelJob: (accountId) => unwrap(worker().cancelJob(accountId)),
  quota: (accountId) => unwrap(worker().quota(accountId)),
};

// Sign out: the database goes with the session. A worker that fails the
// call is named in the console and holds nothing up either.
export function clearCache(): Promise<void> {
  const cleared = worker()
    .clear(TAB)
    .catch((error: unknown) => {
      console.error("cache: clearing failed", error instanceof Error ? error.message : error);
    });
  return Promise.race([cleared, passed(CLEAR_WAIT_MS)]);
}

// Persistent storage is asked for once per page, from the window, since
// a worker cannot ask; the worker writes once the outcome is known, and
// a request that fails counts as an outcome. An instance that keeps
// mail off the disk has nothing to persist and asks for nothing.
async function requestPersistence(strict: boolean): Promise<void> {
  if (persistenceRequested) {
    return;
  }
  persistenceRequested = true;
  try {
    if (!strict && "storage" in navigator) {
      await navigator.storage.persist();
    }
  } catch (error) {
    console.error(
      "cache: persistence request failed",
      error instanceof Error ? error.message : error,
    );
  } finally {
    await worker().persisted();
  }
}

// Asks the worker for a poll now, as a tab's return to view does: what
// a retry brought back is fetched without waiting for the interval.
export function pollCache(): void {
  void worker().focus();
}

// Keeps the worker told about this tab: its lease on mount and at the
// renewal interval, a poll when the tab comes back into view.
export function attachCache(lease: Lease): () => void {
  const renew = (): void => {
    void worker().attach(lease);
  };
  const seen = (): void => {
    if (document.visibilityState === "visible") {
      renew();
      void worker().focus();
    }
  };
  void requestPersistence(lease.strict);
  renew();
  const timer = setInterval(renew, LEASE_RENEW_MS);
  document.addEventListener("visibilitychange", seen);
  return () => {
    clearInterval(timer);
    document.removeEventListener("visibilitychange", seen);
  };
}

function isMailQuery(queryKey: readonly unknown[]): boolean {
  return MAIL_KEY_WORDS.has(queryKey[1]);
}

export type RefusedMessage = Extract<CacheMessage, { kind: "refused" }>;
export type JobMessage = Extract<CacheMessage, { kind: "job" }>;

// What a tab does about a message beyond its queries: end its session
// once another tab cleared the database, say that the server refused a
// change and follow a folder job.
export interface CacheListeners {
  cleared(): void;
  refused(message: RefusedMessage): void;
  job(message: JobMessage): void;
}

// A tree that moved brings the quota with it, which rides the same pass.
// Any move reads the account's searches and senders again: a search from
// its held pages over the rows as they stand now, so no row joins it.
function invalidateChanged(
  queryClient: QueryClient,
  message: Extract<CacheMessage, { kind: "changed" }>,
): void {
  const { accountId } = message;
  const moved = message.mailboxes || message.windows.length > 0 || message.threads.length > 0;
  const keys = [
    ...(moved ? [queryKeys.searches(accountId), queryKeys.senders(accountId)] : []),
    ...(message.mailboxes ? [queryKeys.mailboxes(accountId), queryKeys.quota(accountId)] : []),
    ...message.windows.map((mailboxId) => queryKeys.windows(accountId, mailboxId)),
    ...message.threads.map((threadId) => queryKeys.thread(accountId, threadId)),
  ];
  for (const queryKey of keys) {
    void queryClient.invalidateQueries({ queryKey });
  }
}

// A tab whose session answer says the instance stores mail holds an
// older answer than the worker that keeps it off the disk.
function rereadSetting(queryClient: QueryClient): void {
  if (queryClient.getQueryData(sessionQueryOptions.queryKey)?.privacyStrict === false) {
    void queryClient.invalidateQueries({ queryKey: queryKeys.session });
  }
}

// What a message of the worker asks of this tab's queries; a database
// another tab cleared, a refused change and a job's progress go to the
// caller as well.
export function applyCacheMessage(
  queryClient: QueryClient,
  message: CacheMessage,
  listeners: CacheListeners,
): void {
  switch (message.kind) {
    case "cleared":
      queryClient.removeQueries({ predicate: (query) => isMailQuery(query.queryKey) });
      if (message.by !== TAB) {
        listeners.cleared();
      }
      break;
    case "account":
      void queryClient.invalidateQueries({ queryKey: queryKeys.accounts });
      break;
    case "refused":
      listeners.refused(message);
      break;
    case "reset":
      void queryClient.invalidateQueries({ predicate: (query) => isMailQuery(query.queryKey) });
      break;
    case "strict":
      rereadSetting(queryClient);
      break;
    case "job":
      listeners.job(message);
      break;
    default:
      invalidateChanged(queryClient, message);
  }
}

// Every tab hears what the worker changed, whichever tab's worker did it.
export function installCacheListener(
  queryClient: QueryClient,
  listeners: CacheListeners,
): () => void {
  const channel = new BroadcastChannel(CACHE_CHANNEL);
  channel.addEventListener("message", (event: MessageEvent<unknown>) => {
    const message = readCacheMessage(event.data);
    if (message !== null) {
      applyCacheMessage(queryClient, message, listeners);
    }
  });
  return () => {
    channel.close();
  };
}
