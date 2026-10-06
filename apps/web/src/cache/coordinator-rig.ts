// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { ACCOUNT, FakeJmap, at, email, mailbox } from "@huliho/core/testing";
import { IDBFactory, IDBKeyRange } from "fake-indexeddb";
import { expect, vi } from "vitest";

import { CHANGES_POLL_MS, Coordinator } from "./coordinator";
import type { CacheApi } from "./coordinator";
import { DexieMailStore, MailDatabase } from "./db";
import { webLocks } from "./locks";
import type { Locks } from "./locks";
import type { CacheMessage } from "./messages";

// The rig the coordinator tests run in: a fake server, a store on a
// fresh IndexedDB per test and the platform's locks under a name per
// test, so a lock a test leaves held never reaches the next one.

// Only the clock is faked: IndexedDB runs on immediates and the fake
// server answers on real promises.
const FAKED = ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"] as const;

// How many turns of the event loop let the work of a fired timer finish.
const DRAIN_TURNS = 50;

export interface Rig {
  server: FakeJmap;
  posted: CacheMessage[];
  coordinator: Coordinator;
  tab: CacheApi;
}

// One IndexedDB per test, shared by every coordinator of the test as the
// tabs of one browser share it.
let indexedDB = new IDBFactory();
let scope = "";

export function beginRig(): void {
  scope = crypto.randomUUID();
  vi.useFakeTimers({ toFake: [...FAKED] });
}

export function endRig(): void {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  indexedDB = new IDBFactory();
}

export function store(): DexieMailStore {
  return new DexieMailStore(new MailDatabase("huliho-test", { indexedDB, IDBKeyRange }));
}

function locks(): Locks {
  const platform = webLocks(navigator.locks);
  return { request: (name, run) => platform.request(`${scope}:${name}`, run) };
}

// A server with an inbox and an archive and `count` emails in the inbox.
export function serve(count: number): FakeJmap {
  const server = new FakeJmap();
  server.putMailbox(mailbox("inbox", "inbox"));
  server.putMailbox(mailbox("archive", "archive"));
  for (let index = 1; index <= count; index += 1) {
    server.addEmail(email(`e${String(index)}`, { receivedAt: at(index) }));
  }
  vi.stubGlobal("fetch", server.fetch);
  return server;
}

export function coordinate(posted: CacheMessage[]): Coordinator {
  return new Coordinator({
    store: store(),
    choose: () => "kept",
    locks: locks(),
    post: (message) => {
      posted.push(message);
    },
  });
}

export function posts(posted: CacheMessage[], count: number): () => void {
  return () => {
    expect(posted).toHaveLength(count);
  };
}

export function requests(server: FakeJmap, count: number): () => void {
  return () => {
    expect(server.posted()).toHaveLength(count);
  };
}

// Waits for a poll or a forget to land, without moving the clock past
// the next poll.
export function settled(check: () => void): Promise<void> {
  return vi.waitFor(check);
}

export function drained(turns = DRAIN_TURNS): Promise<void> {
  if (turns === 0) {
    return Promise.resolve();
  }
  return new Promise((resolve) => {
    setImmediate(resolve);
  }).then(() => drained(turns - 1));
}

// Runs the given number of polls, each to its end.
export async function polled(posted: CacheMessage[], count: number): Promise<void> {
  if (count === 0) {
    return;
  }
  const expected = posted.length + 1;
  await vi.advanceTimersByTimeAsync(CHANGES_POLL_MS);
  await settled(posts(posted, expected));
  await polled(posted, count - 1);
}

// One tab attached to one account on a server of `count` emails, its
// tree landed.
export async function attached(count: number): Promise<Rig> {
  const server = serve(count);
  const posted: CacheMessage[] = [];
  const coordinator = coordinate(posted);
  const tab = coordinator.api();
  await tab.persisted();
  await tab.attach({ accounts: [ACCOUNT], listedAt: Date.now(), watching: null, strict: false });
  await settled(posts(posted, 1));
  return { server, posted, coordinator, tab };
}
