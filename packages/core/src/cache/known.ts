// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailHeader } from "../jmap/schemas";
import { memberOf } from "./members";
import type { MailStore, MemberState } from "./store";

// An email a change names, with the thread it stands in where the
// caller knows it.
export interface EmailRef {
  id: string;
  threadId?: string | undefined;
}

// What the store knows of an email: its header where it holds one, else
// its member in the thread the change names. A list keeps the header of
// each thread's exemplar alone, so the other emails of a thread stand in
// its row until the thread opens.
export interface KnownEmail {
  threadId: string;
  // When the server received it; null without a header.
  receivedAt: string | null;
  state: MemberState;
  header: EmailHeader | null;
}

export async function knownEmails(
  store: MailStore,
  accountId: string,
  refs: readonly EmailRef[],
): Promise<Map<string, KnownEmail>> {
  const headers = await store.emails(accountId, [...new Set(refs.map(({ id }) => id))]);
  const known = new Map(
    [...headers.values()].map((header): [string, KnownEmail] => [
      header.id,
      { threadId: header.threadId, receivedAt: header.receivedAt, state: memberOf(header), header },
    ]),
  );
  const rest = refs.flatMap(({ id, threadId }) =>
    known.has(id) || threadId === undefined ? [] : [{ id, threadId }],
  );
  const threads = await store.threads(accountId, [
    ...new Set(rest.map(({ threadId }) => threadId)),
  ]);
  for (const { id, threadId } of rest) {
    const thread = threads.get(threadId);
    const state =
      thread === undefined ? undefined : new Map(Object.entries(thread.members)).get(id);
    if (state !== undefined && !known.has(id)) {
      known.set(id, { threadId, receivedAt: null, state, header: null });
    }
  }
  return known;
}
