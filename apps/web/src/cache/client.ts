// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { JmapError } from "@huliho/core";
import type { MailCache } from "@huliho/core";
import { MAIL_KEY_WORDS, queryKeys } from "@huliho/state";
import type { QueryClient } from "@tanstack/react-query";
import { wrap } from "comlink";
import type { Remote } from "comlink";

import { LEASE_RENEW_MS } from "./coordinator";
import type { CacheApi, Lease } from "./coordinator";
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
// query hooks read, its cause and limit intact.
async function unwrap<Value>(answer: Promise<CacheResult<Value>>): Promise<Value> {
  const result = await answer;
  if (result.ok) {
    return result.value;
  }
  const { failure } = result;
  throw new JmapError(failure.code, {
    ...(failure.stopCause === null ? {} : { stopCause: failure.stopCause }),
    ...(failure.limit === null ? {} : { limit: failure.limit }),
  });
}

// The cache as the query hooks read it, served by the worker.
export const mailCache: MailCache = {
  mailboxes: (accountId) => unwrap(worker().mailboxes(accountId)),
  window: (accountId, mailboxId, page) => unwrap(worker().window(accountId, mailboxId, page)),
  thread: (accountId, threadId) => unwrap(worker().thread(accountId, threadId)),
  reveal: (accountId, mailboxId) => unwrap(worker().reveal(accountId, mailboxId)),
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
// a request that fails counts as an outcome.
async function requestPersistence(): Promise<void> {
  if (persistenceRequested) {
    return;
  }
  persistenceRequested = true;
  try {
    if ("storage" in navigator) {
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
  void requestPersistence();
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

// A change lands as an invalidation of the queries it names. A cleared
// database takes every mail query with it and, when another tab did it,
// hands the rest to the caller, since the session that owned it ended.
export function applyCacheMessage(
  queryClient: QueryClient,
  message: CacheMessage,
  onCleared: () => void,
): void {
  if (message.kind === "cleared") {
    queryClient.removeQueries({ predicate: (query) => isMailQuery(query.queryKey) });
    if (message.by !== TAB) {
      onCleared();
    }
    return;
  }
  const { accountId } = message;
  const keys = [
    ...(message.mailboxes ? [queryKeys.mailboxes(accountId)] : []),
    ...message.windows.map((mailboxId) => queryKeys.windows(accountId, mailboxId)),
    ...message.threads.map((threadId) => queryKeys.thread(accountId, threadId)),
  ];
  for (const queryKey of keys) {
    void queryClient.invalidateQueries({ queryKey });
  }
}

// Every tab hears what the worker changed, whichever tab's worker did it.
export function installCacheListener(queryClient: QueryClient, onCleared: () => void): () => void {
  const channel = new BroadcastChannel(CACHE_CHANNEL);
  channel.addEventListener("message", (event: MessageEvent<unknown>) => {
    const message = readCacheMessage(event.data);
    if (message !== null) {
      applyCacheMessage(queryClient, message, onCleared);
    }
  });
  return () => {
    channel.close();
  };
}
