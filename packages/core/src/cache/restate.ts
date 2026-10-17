// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailHeader } from "../jmap/schemas";
import { movesOf, recounted } from "./counts";
import type { KnownEmail } from "./known";
import { listsAfter } from "./lists";
import type { ListChange } from "./lists";
import type { AppliedChanges } from "./poll";
import type { Batch, MailStore, MemberState, ThreadRow } from "./store";

// The thread row with the state of the members that changed.
function withMembers(thread: ThreadRow, changed: ReadonlyMap<string, MemberState>): ThreadRow {
  return {
    ...thread,
    members: Object.fromEntries(
      Object.entries(thread.members).map(([id, member]) => [id, changed.get(id) ?? member]),
    ),
  };
}

// The rows as they stand once some emails take other states: their
// headers, the members of their threads, the counts of their mailboxes
// and the lists they leave. `known` holds what the store knew of each
// email before; a list takes a thread back where `restores` names one
// of its emails.
export async function restate(
  store: MailStore,
  accountId: string,
  states: ReadonlyMap<string, MemberState>,
  { known, restores }: { known: ReadonlyMap<string, KnownEmail>; restores: ReadonlySet<string> },
): Promise<{ batch: Batch; changes: AppliedChanges }> {
  const changes = [...states].flatMap(([id, after]): ListChange[] => {
    const email = known.get(id);
    return email === undefined
      ? []
      : [
          {
            id,
            threadId: email.threadId,
            receivedAt: email.receivedAt,
            before: email.state,
            after,
            restores: restores.has(id),
          },
        ];
  });
  const headers = changes.flatMap(({ id, after }): EmailHeader[] => {
    const header = known.get(id)?.header;
    return header === undefined || header === null ? [] : [{ ...header, ...after }];
  });
  const threadIds = [...new Set(changes.map((change) => change.threadId))];
  const threads = [...(await store.threads(accountId, threadIds)).values()];
  const members = new Map(threads.map((thread) => [thread.id, thread.members]));
  const mailboxes = recounted(await store.mailboxes(accountId), movesOf(members, changes));
  // Keywords alone move no list, so the lists are read for a move alone.
  const moving = changes.some(
    (change) =>
      JSON.stringify(change.before.mailboxIds) !== JSON.stringify(change.after.mailboxIds),
  );
  const lists = moving ? await listsAfter(store, accountId, changes, members) : [];
  const after = new Map(changes.map((change) => [change.id, change.after]));
  return {
    batch: {
      emails: { put: headers },
      threads: { put: threads.map((thread) => withMembers(thread, after)) },
      mailboxes: { put: mailboxes },
      queries: { put: lists },
    },
    changes: {
      mailboxes: mailboxes.length > 0,
      windows: [
        ...new Set(
          changes.flatMap((change) =>
            Object.keys(change.before.mailboxIds).concat(Object.keys(change.after.mailboxIds)),
          ),
        ),
      ],
      threads: threadIds,
    },
  };
}
