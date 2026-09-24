// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { answer, callsFor } from "../jmap/calls";
import type { ObjectType } from "../jmap/calls";
import type { JmapClient } from "../jmap/client";
import {
  HEADER_PROPERTIES,
  emailHeaderSchema,
  getAnswerSchema,
  threadSchema,
} from "../jmap/schemas";
import type { EmailHeader } from "../jmap/schemas";
import type { ThreadDetail } from "./api";
import { PREVIEW_BATCH } from "./limits";
import { fetchInChunks, stateAnswerSchema, threadRow } from "./members";
import type { MailStore, ThreadRow } from "./store";

const threadAnswerSchema = getAnswerSchema(threadSchema);

// The thread as the server names it now, and the states an account
// without any starts from.
interface FetchedThread {
  thread: ThreadRow;
  states: Partial<Record<ObjectType, string>>;
}

// The thread row from the server, for one the store does not hold: a
// deep link before the list fetched it. While the account holds no
// states, two empty gets ahead take the states every later /changes
// starts from, as a window fetch does. Null when the server has no
// such thread.
async function fetchThread(
  client: JmapClient,
  store: MailStore,
  threadId: string,
): Promise<FetchedThread | null> {
  const session = await client.session();
  const calls = callsFor(session.accountId);
  const leading = (await store.state(client.accountId, "Email")) === null;
  const responses = await client.request([
    ...(leading ? [calls.get("Email", [], "e0"), calls.get("Thread", [], "t0")] : []),
    calls.get("Thread", [threadId], "t"),
  ]);
  const thread = answer(responses, "t", threadAnswerSchema).list.find((row) => row.id === threadId);
  if (thread === undefined) {
    return null;
  }
  return {
    thread: threadRow(thread, new Map()),
    states: leading
      ? {
          Email: answer(responses, "e0", stateAnswerSchema).state,
          Thread: answer(responses, "t0", threadAnswerSchema).state,
        }
      : {},
  };
}

// The headers of the members named, with their previews, in gets of a
// preview batch at most; a bridge fills the previews on demand.
function fetchHeaders(client: JmapClient, ids: readonly string[]): Promise<EmailHeader[]> {
  if (ids.length === 0) {
    return Promise.resolve([]);
  }
  const wanted = {
    type: "Email" as const,
    ids,
    properties: HEADER_PROPERTIES,
    batch: PREVIEW_BATCH,
  };
  return fetchInChunks(client, wanted, emailHeaderSchema);
}

// The thread as the reading pane opens it: its row and every member's
// header. A member the store holds no header for, or whose preview is
// still empty, is fetched with its preview; a thread the store does not
// hold is fetched first. Null for a thread the server does not have.
export async function readThread(
  client: JmapClient,
  store: MailStore,
  threadId: string,
): Promise<ThreadDetail | null> {
  const { accountId } = client;
  const known = (await store.threads(accountId, [threadId])).get(threadId);
  const fetched = known === undefined ? await fetchThread(client, store, threadId) : null;
  const thread = known ?? fetched?.thread;
  if (thread === undefined) {
    return null;
  }
  const held = await store.emails(accountId, thread.emailIds);
  const wanted = thread.emailIds.filter((id) => (held.get(id)?.preview ?? "") === "");
  const headers = await fetchHeaders(client, wanted);
  for (const header of headers) {
    held.set(header.id, header);
  }
  // The row learns the state of every member it lacked.
  const row = threadRow(thread, new Map(headers.map((header) => [header.id, header])), thread);
  if (headers.length > 0 || fetched !== null) {
    await store.commit(accountId, {
      emails: { put: headers },
      threads: { put: [row] },
      states: fetched?.states ?? {},
    });
  }
  return {
    thread: row,
    emails: Object.fromEntries(
      thread.emailIds.flatMap((id): [string, EmailHeader][] => {
        const header = held.get(id);
        return header === undefined ? [] : [[id, header]];
      }),
    ),
  };
}
