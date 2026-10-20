// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { JobProgress } from "@huliho/core";
import { useSyncExternalStore } from "react";

// A running job not heard of for this long counts as gone, as when the
// tab whose worker ran it closed. A round asks twice and may wait for a
// poll first, each up to the bridge's minute per request.
export const JOB_LAPSE_MS = 180_000;

type JobEnd = (accountId: string, progress: JobProgress) => void;

interface Facts {
  // The running job of each account, as the worker last told it.
  running: ReadonlyMap<string, JobProgress>;
  // The accounts where a job was asked while the device was offline.
  offline: ReadonlySet<string>;
}

let facts: Facts = { running: new Map(), offline: new Set() };
const listeners = new Set<() => void>();
const enders = new Set<JobEnd>();
const lapses = new Map<string, ReturnType<typeof setTimeout>>();

function publish(next: Facts): void {
  facts = next;
  for (const listener of listeners) {
    listener();
  }
}

function without<Value>(map: ReadonlyMap<string, Value>, key: string): Map<string, Value> {
  const next = new Map(map);
  next.delete(key);
  return next;
}

function stopLapse(accountId: string): void {
  clearTimeout(lapses.get(accountId));
  lapses.delete(accountId);
}

// Where a folder job stands, as the worker told every tab: a running job
// stands until it ends or falls silent; its end goes to every listener.
export function heardJob(accountId: string, progress: JobProgress): void {
  stopLapse(accountId);
  if (progress.state !== "running") {
    publish({ ...facts, running: without(facts.running, accountId) });
    for (const end of enders) {
      end(accountId, progress);
    }
    return;
  }
  lapses.set(
    accountId,
    setTimeout(() => {
      lapses.delete(accountId);
      publish({ ...facts, running: without(facts.running, accountId) });
    }, JOB_LAPSE_MS),
  );
  const offline = new Set(facts.offline);
  offline.delete(accountId);
  publish({ running: new Map(facts.running).set(accountId, progress), offline });
}

export function onJobEnd(end: JobEnd): () => void {
  enders.add(end);
  return () => {
    enders.delete(end);
  };
}

function backOnline(): void {
  window.removeEventListener("online", backOnline);
  publish({ ...facts, offline: new Set() });
}

// A job asked offline runs nothing; the header says so until the
// connection is back.
export function askedOffline(accountId: string): void {
  window.addEventListener("online", backOnline);
  publish({ ...facts, offline: new Set(facts.offline).add(accountId) });
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function read(): Facts {
  return facts;
}

// The account's running job, null while none runs.
export function useRunningJob(accountId: string): JobProgress | null {
  return useSyncExternalStore(subscribe, read, read).running.get(accountId) ?? null;
}

// Whether a job of the account was asked while offline.
export function useAskedOffline(accountId: string): boolean {
  return useSyncExternalStore(subscribe, read, read).offline.has(accountId);
}
