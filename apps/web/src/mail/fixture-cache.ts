// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type {
  BodyDetail,
  EmailHeader,
  Job,
  ListPage,
  MailCache,
  Mutation,
  PatchMutation,
  ThreadDetail,
  Undo,
} from "@huliho/core";

import { previewDetail } from "./body-fixtures";
import { MAILBOXES } from "./fixtures";

// How a fixture cache answers a page: with rows, with a refusal or not at all.
export type PageAnswer = ListPage | "never" | Error;
// How it answers a thread: with the thread, with nothing, with a refusal or not at all.
export type ThreadAnswer = ThreadDetail | null | "never" | Error;
// How it answers a body: with the body, with nothing, with a refusal or not at all.
export type BodyAnswer = BodyDetail | null | "never" | Error;

function answered<Value>(answer: Value | "never" | Error): Promise<Value> {
  if (answer === "never") {
    return new Promise(() => undefined);
  }
  return answer instanceof Error ? Promise.reject(answer) : Promise.resolve(answer);
}

// The header of an email among the threads held, if any.
function headerOf(threads: ReadonlyMap<string, ThreadAnswer>, emailId: string): EmailHeader | null {
  for (const answer of threads.values()) {
    if (answer !== null && answer !== "never" && !(answer instanceof Error)) {
      const found = new Map(Object.entries(answer.emails)).get(emailId);
      if (found !== undefined) {
        return found;
      }
    }
  }
  return null;
}

// What a fixture cache records beside answering.
export interface Recorded {
  revealed: string[];
  mutations: PatchMutation[];
  // Each call of `mutateMany`, with the held destroys released and
  // dropped and the jobs started.
  batches: Mutation[][];
  released: number[][];
  dropped: number[][];
  jobs: Job[];
}

// A patch with its values turned over, as the change that takes it back.
function flipped(mutation: PatchMutation): PatchMutation {
  const inverse: PatchMutation = {
    type: "Email",
    id: mutation.id,
    patch: Object.fromEntries(
      Object.entries(mutation.patch).map(([path, value]) => [path, value === true ? null : true]),
    ),
    restores: true,
  };
  if (mutation.threadId !== undefined) {
    inverse.threadId = mutation.threadId;
  }
  return inverse;
}

// The answer a fixture gives a batch: each patch turned over, each
// destroy held under the next number.
function undoOf(mutations: readonly Mutation[], first: number): Undo {
  const patches = mutations.flatMap((mutation) => ("patch" in mutation ? [mutation] : []));
  const destroys = mutations.filter((mutation) => "destroy" in mutation);
  return {
    inverses: patches.map(flipped),
    held: destroys.map((_mutation, index) => first + index),
    threads: [...new Set(mutations.flatMap((mutation) => mutation.threadId ?? []))],
  };
}

// The changes, the jobs and the quota of a fixture cache, each a no-op
// it records.
function recordedWrites(
  recorded: Recorded,
): Pick<MailCache, "mutate" | "mutateMany" | "release" | "drop" | "job" | "cancelJob" | "quota"> {
  return {
    mutate: (_accountId, mutation) => {
      recorded.mutations.push(mutation);
      return Promise.resolve();
    },
    mutateMany: (_accountId, batch) => {
      const first = recorded.batches.flat().filter((mutation) => "destroy" in mutation).length;
      recorded.batches.push([...batch]);
      return Promise.resolve(undoOf(batch, first + 1));
    },
    release: (_accountId, seqs) => {
      recorded.released.push([...seqs]);
      return Promise.resolve();
    },
    drop: (_accountId, seqs) => {
      recorded.dropped.push([...seqs]);
      return Promise.resolve();
    },
    job: (_accountId, job) => {
      recorded.jobs.push(job);
      return Promise.resolve();
    },
    cancelJob: () => Promise.resolve(),
    quota: () => Promise.resolve(null),
  };
}

// A cache for stories and tests: pages by mailbox and number, threads
// by id and bodies by email id, the key `<id>#large` for the ask at the
// large cap; a message without a body of its own shows its preview as
// text. The mailbox tree comes from the fixtures; every reveal and every
// change is a no-op it records.
export function fixtureCache(
  pages: Record<string, PageAnswer>,
  threads: Record<string, ThreadAnswer> = {},
  bodies: Record<string, BodyAnswer> = {},
): MailCache & Recorded {
  const recorded: Recorded = {
    revealed: [],
    mutations: [],
    batches: [],
    released: [],
    dropped: [],
    jobs: [],
  };
  const held = new Map(Object.entries(threads));
  const heldBodies = new Map(Object.entries(bodies));
  return {
    ...recorded,
    ...recordedWrites(recorded),
    mailboxes: () => Promise.resolve(MAILBOXES),
    window: (_accountId, mailboxId, page) => {
      const answer = pages[`${mailboxId}/${String(page)}`];
      return answer === undefined
        ? Promise.resolve({ rows: [], total: 0, pending: 0 })
        : answered(answer);
    },
    thread: (_accountId, threadId) => answered(held.get(threadId) ?? null),
    reveal: (_accountId, mailboxId) => {
      recorded.revealed.push(mailboxId);
      return Promise.resolve();
    },
    body: (_accountId, emailId, { large }) => {
      const answer =
        (large ? heldBodies.get(`${emailId}#large`) : undefined) ?? heldBodies.get(emailId);
      if (answer !== undefined) {
        return answered(answer);
      }
      const found = headerOf(held, emailId);
      return Promise.resolve(found === null ? null : previewDetail(found));
    },
  };
}
