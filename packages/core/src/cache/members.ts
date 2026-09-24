// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { answer, callsFor } from "../jmap/calls";
import type { ObjectType } from "../jmap/calls";
import type { JmapClient } from "../jmap/client";
import { STATE_PROPERTIES, emailStateSchema, getAnswerSchema } from "../jmap/schemas";
import type { EmailState, Thread } from "../jmap/schemas";
import type { z } from "../schema";
import type { MemberState, ThreadRow } from "./store";

export const stateAnswerSchema = getAnswerSchema(emailStateSchema);

export function memberOf(state: EmailState): MemberState {
  return { keywords: state.keywords, mailboxIds: state.mailboxIds };
}

// The thread's row: its emails in the server's order, each with the
// state an answer holds or the state the row held before.
export function threadRow(
  thread: Thread,
  states: ReadonlyMap<string, EmailState>,
  held?: ThreadRow,
): ThreadRow {
  const before = new Map(Object.entries(held?.members ?? {}));
  const members = thread.emailIds.flatMap((id): [string, MemberState][] => {
    const state = states.get(id);
    if (state !== undefined) {
      return [[id, memberOf(state)]];
    }
    const kept = before.get(id);
    return kept === undefined ? [] : [[id, kept]];
  });
  return { id: thread.id, emailIds: thread.emailIds, members: Object.fromEntries(members) };
}

function chunked<Item>(items: readonly Item[], size: number): Item[][] {
  const chunks: Item[][] = [];
  for (let start = 0; start < items.length; start += size) {
    chunks.push(items.slice(start, start + size));
  }
  return chunks;
}

interface Wanted {
  type: ObjectType;
  ids: readonly string[];
  properties?: readonly string[];
  // A cap on one get below the server's, where the caller has one.
  batch?: number;
}

// The objects named, in the round trips the server's limits ask for. A
// window and a poll ask them by result reference first; this is the
// road when that answer was too large, and the road a thread's members
// take.
export async function fetchInChunks<Schema extends z.ZodType>(
  client: JmapClient,
  wanted: Wanted,
  item: Schema,
): Promise<z.output<Schema>[]> {
  const session = await client.session();
  const calls = callsFor(session.accountId);
  const schema = getAnswerSchema(item);
  const size = Math.min(session.maxObjectsInGet, wanted.batch ?? session.maxObjectsInGet);
  const chunks = chunked([...new Set(wanted.ids)], size);
  const batches = chunked(chunks, session.maxCallsInRequest);
  const answered = await Promise.all(
    batches.map(async (batch) => {
      const responses = await client.request(
        batch.map((chunk, index) =>
          calls.get(wanted.type, chunk, `s${String(index)}`, wanted.properties),
        ),
      );
      return batch.flatMap((_chunk, index) => answer(responses, `s${String(index)}`, schema).list);
    }),
  );
  return answered.flat();
}

export async function fetchStates(
  client: JmapClient,
  ids: readonly string[],
): Promise<Map<string, EmailState>> {
  const wanted = { type: "Email" as const, ids, properties: STATE_PROPERTIES };
  const states = await fetchInChunks(client, wanted, emailStateSchema);
  return new Map(states.map((state) => [state.id, state]));
}
