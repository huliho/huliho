// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { JmapError } from "@huliho/core";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import {
  BODY_FETCHES_IN_FLIGHT,
  BODY_LIMIT_RETRY_MS,
  BODY_LIMIT_TRIES,
  BodyQueue,
} from "./body-queue";

const ACCOUNT = "acc-1";
const OTHER = "acc-2";

// The proxy's answer to a request past the account's concurrency.
function busy(): JmapError {
  return new JmapError("limit", { limit: "maxConcurrentRequests" });
}

// A fetch the test settles by hand, with its place in the order it was asked.
interface Held {
  settle: PromiseWithResolvers<string>;
  started: boolean;
}

function held(): Held {
  return { settle: Promise.withResolvers<string>(), started: false };
}

// Asks the queue for `count` bodies of one account, each waiting on the test.
function ask(queue: BodyQueue, accountId: string, count: number): [Held[], Promise<string>[]] {
  const fetches = Array.from({ length: count }, held);
  const answers = fetches.map((fetch) =>
    queue.read(accountId, () => {
      fetch.started = true;
      return fetch.settle.promise;
    }),
  );
  return [fetches, answers];
}

function started(fetches: readonly Held[]): boolean[] {
  return fetches.map((fetch) => fetch.started);
}

async function flushed(): Promise<void> {
  await vi.advanceTimersByTimeAsync(0);
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

test("two fetches of an account run at once and the rest wait in the order they were asked", async () => {
  const queue = new BodyQueue();
  const [fetches, answers] = ask(queue, ACCOUNT, 4);
  expect(started(fetches)).toEqual([true, true, false, false]);
  fetches[1]?.settle.resolve("second");
  await flushed();
  expect(started(fetches)).toEqual([true, true, true, false]);
  expect(await answers[1]).toBe("second");
  fetches[0]?.settle.resolve("first");
  await flushed();
  expect(started(fetches)).toEqual([true, true, true, true]);
  expect(fetches.filter((fetch) => fetch.started)).toHaveLength(BODY_FETCHES_IN_FLIGHT + 2);
});

test("each account has a lane of its own", () => {
  const queue = new BodyQueue();
  const [first] = ask(queue, ACCOUNT, 3);
  const [second] = ask(queue, OTHER, 3);
  expect(started(first)).toEqual([true, true, false]);
  expect(started(second)).toEqual([true, true, false]);
});

test("a limit answer goes back to the head of the line and the lane waits before it tries again", async () => {
  const queue = new BodyQueue();
  const runs: string[] = [];
  let limited = false;
  const refused = queue.read(ACCOUNT, () => {
    runs.push("refused");
    if (!limited) {
      limited = true;
      return Promise.reject(busy());
    }
    return Promise.resolve("landed");
  });
  const [fetches] = ask(queue, ACCOUNT, 2);
  await flushed();
  // The second lane slot ran one fetch; the lane then holds the third.
  expect(runs).toEqual(["refused"]);
  expect(started(fetches)).toEqual([true, false]);
  await vi.advanceTimersByTimeAsync(BODY_LIMIT_RETRY_MS - 1);
  expect(runs).toEqual(["refused"]);
  await vi.advanceTimersByTimeAsync(1);
  expect(runs).toEqual(["refused", "refused"]);
  expect(await refused).toBe("landed");
  expect(started(fetches)).toEqual([true, true]);
});

test("a fetch the limit keeps refusing is tried a bounded number of times, then the refusal reaches the caller", async () => {
  const queue = new BodyQueue();
  let runs = 0;
  const refused = queue.read(ACCOUNT, () => {
    runs += 1;
    return Promise.reject(busy());
  });
  const outcome = refused.catch((error: unknown) => error);
  await vi.advanceTimersByTimeAsync(BODY_LIMIT_TRIES * BODY_LIMIT_RETRY_MS);
  expect(runs).toBe(BODY_LIMIT_TRIES);
  const error = await outcome;
  expect(error).toBeInstanceOf(JmapError);
  expect(error instanceof JmapError && error.code).toBe("limit");
  // The line moves on: the next fetch of the account runs at once.
  expect(await queue.read(ACCOUNT, () => Promise.resolve("next"))).toBe("next");
});

test("any other failure reaches the caller as it is, a limit that time does not lift among them", async () => {
  const queue = new BodyQueue();
  const failure = new JmapError("unavailable");
  await expect(queue.read(ACCOUNT, () => Promise.reject(failure))).rejects.toBe(failure);
  const plain = new Error("broken");
  await expect(queue.read(ACCOUNT, () => Promise.reject(plain))).rejects.toBe(plain);
  const tooLarge = new JmapError("limit", { limit: "maxSizeRequest" });
  await expect(queue.read(ACCOUNT, () => Promise.reject(tooLarge))).rejects.toBe(tooLarge);
  const unnamed = new JmapError("limit");
  await expect(queue.read(ACCOUNT, () => Promise.reject(unnamed))).rejects.toBe(unnamed);
});

test("stopping an account ends the fetches that wait and lets the ones in flight land", async () => {
  const queue = new BodyQueue();
  const [fetches, answers] = ask(queue, ACCOUNT, 3);
  queue.stop(ACCOUNT);
  await expect(answers[2]).rejects.toMatchObject({ code: "unavailable" });
  expect(started(fetches)).toEqual([true, true, false]);
  // An account that comes back has a lane of its own, whatever the old one still runs.
  const [again] = ask(queue, ACCOUNT, 3);
  expect(started(again)).toEqual([true, true, false]);
  fetches[0]?.settle.resolve("first");
  expect(await answers[0]).toBe("first");
  await flushed();
  expect(started(again)).toEqual([true, true, false]);
});

test("stopping an account on hold drops the hold with the line", async () => {
  const queue = new BodyQueue();
  const refused = queue.read(ACCOUNT, () => Promise.reject(busy()));
  await flushed();
  queue.stop(ACCOUNT);
  await expect(refused).rejects.toMatchObject({ code: "unavailable" });
  await vi.advanceTimersByTimeAsync(BODY_LIMIT_RETRY_MS);
  const [again] = ask(queue, ACCOUNT, 1);
  expect(started(again)).toEqual([true]);
});

test("a fetch in flight that meets the limit after its account left is ended, never tried again", async () => {
  const queue = new BodyQueue();
  const settle = Promise.withResolvers<string>();
  let runs = 0;
  const answer = queue.read(ACCOUNT, () => {
    runs += 1;
    return settle.promise;
  });
  queue.stop(ACCOUNT);
  settle.reject(busy());
  await expect(answer).rejects.toMatchObject({ code: "unavailable" });
  await vi.advanceTimersByTimeAsync(BODY_LIMIT_RETRY_MS);
  expect(runs).toBe(1);
});
