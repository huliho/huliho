// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { JmapError } from "@huliho/core";

// The body fetches one account runs at once: with the poll and the
// flush beside them a thread of many unread cards stays under the
// proxy's four requests per account.
export const BODY_FETCHES_IN_FLIGHT = 2;

// A fetch the proxy refused for its limit waits this long before the
// lane tries it again.
export const BODY_LIMIT_RETRY_MS = 1000;

// How often the lane sends one fetch the limit keeps refusing. The
// last refusal reaches the caller, so a card offers Try again and never
// waits on a server that stays busy.
export const BODY_LIMIT_TRIES = 5;

// The one limit that time lifts: too many requests of the account at
// once (RFC 8620 section 3.6.1). A request past any other limit fails
// the same way every time.
const CONCURRENCY_LIMIT = "maxConcurrentRequests";

// One fetch as the lane runs it: an attempt settles the caller itself
// and throws only for a limit answer the lane may try again; a fetch
// abandoned in line is settled with a refusal.
interface Fetch {
  attempt: () => Promise<void>;
  abandon: () => void;
}

// One account's fetches: the ones running, the ones waiting in the
// order they were asked, the pause a limit answer put on the lane and
// whether the account left while a fetch was out.
interface Lane {
  running: number;
  waiting: Fetch[];
  pause: ReturnType<typeof setTimeout> | null;
  stopped: boolean;
}

function isRetried(error: unknown): boolean {
  return error instanceof JmapError && error.code === "limit" && error.limit === CONCURRENCY_LIMIT;
}

// The body fetches of every account, a few in flight per account and
// the rest in line: the cards ask in the order they stand, so the one
// in view goes first. A limit answer puts the fetch back at the head of
// its line, a bounded number of times.
export class BodyQueue {
  private readonly lanes = new Map<string, Lane>();

  read<Value>(accountId: string, run: () => Promise<Value>): Promise<Value> {
    return new Promise<Value>((resolve, reject) => {
      let tries = 0;
      const attempt = async (): Promise<void> => {
        tries += 1;
        try {
          resolve(await run());
        } catch (error) {
          if (isRetried(error) && tries < BODY_LIMIT_TRIES) {
            throw error;
          }
          reject(error instanceof Error ? error : new Error(String(error)));
        }
      };
      const lane = this.lane(accountId);
      lane.waiting.push({
        attempt,
        abandon: () => {
          reject(new JmapError("unavailable"));
        },
      });
      this.drain(accountId, lane);
    });
  }

  // Ends the fetches of an account that left: the ones in line now and
  // one in flight that comes back for another try. An answer on its way
  // lands as it is.
  stop(accountId: string): void {
    const lane = this.lanes.get(accountId);
    if (lane === undefined) {
      return;
    }
    this.lanes.delete(accountId);
    lane.stopped = true;
    if (lane.pause !== null) {
      clearTimeout(lane.pause);
      lane.pause = null;
    }
    for (const fetch of lane.waiting.splice(0)) {
      fetch.abandon();
    }
  }

  private lane(accountId: string): Lane {
    const held = this.lanes.get(accountId);
    if (held !== undefined) {
      return held;
    }
    const lane: Lane = { running: 0, waiting: [], pause: null, stopped: false };
    this.lanes.set(accountId, lane);
    return lane;
  }

  // An idle lane is forgotten; a stopped one left the map already.
  private forget(accountId: string, lane: Lane): void {
    const idle = lane.running === 0 && lane.waiting.length === 0 && lane.pause === null;
    if (idle && this.lanes.get(accountId) === lane) {
      this.lanes.delete(accountId);
    }
  }

  private drain(accountId: string, lane: Lane): void {
    while (lane.pause === null && lane.running < BODY_FETCHES_IN_FLIGHT) {
      const next = lane.waiting.shift();
      if (next === undefined) {
        break;
      }
      lane.running += 1;
      void this.run(accountId, lane, next);
    }
    this.forget(accountId, lane);
  }

  private async run(accountId: string, lane: Lane, fetch: Fetch): Promise<void> {
    try {
      await fetch.attempt();
    } catch {
      this.retry(accountId, lane, fetch);
    } finally {
      lane.running -= 1;
      this.drain(accountId, lane);
    }
  }

  // The fetch goes back to the head of its line and the lane waits
  // before it sends anything more.
  private retry(accountId: string, lane: Lane, fetch: Fetch): void {
    if (lane.stopped) {
      fetch.abandon();
      return;
    }
    lane.waiting.unshift(fetch);
    lane.pause ??= setTimeout(() => {
      lane.pause = null;
      this.drain(accountId, lane);
    }, BODY_LIMIT_RETRY_MS);
  }
}
