// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type {
  BodyDetail,
  EmailHeader,
  ListPage,
  MailCache,
  Mutation,
  ThreadDetail,
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

// A cache for stories and tests: pages by mailbox and number, threads
// by id and bodies by email id, the key `<id>#large` for the ask at the
// large cap; a message without a body of its own shows its preview as
// text. The mailbox tree comes from the fixtures; every reveal and every
// change is a no-op it records.
export function fixtureCache(
  pages: Record<string, PageAnswer>,
  threads: Record<string, ThreadAnswer> = {},
  bodies: Record<string, BodyAnswer> = {},
): MailCache & { revealed: string[]; mutations: Mutation[] } {
  const revealed: string[] = [];
  const mutations: Mutation[] = [];
  const held = new Map(Object.entries(threads));
  const heldBodies = new Map(Object.entries(bodies));
  return {
    revealed,
    mutations,
    mailboxes: () => Promise.resolve(MAILBOXES),
    window: (_accountId, mailboxId, page) => {
      const answer = pages[`${mailboxId}/${String(page)}`];
      return answer === undefined
        ? Promise.resolve({ rows: [], total: 0, pending: 0 })
        : answered(answer);
    },
    thread: (_accountId, threadId) => answered(held.get(threadId) ?? null),
    reveal: (_accountId, mailboxId) => {
      revealed.push(mailboxId);
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
    mutate: (_accountId, mutation) => {
      mutations.push(mutation);
      return Promise.resolve();
    },
  };
}
