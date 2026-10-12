// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { strip } from "./lists";
import type { AppliedChanges } from "./poll";
import type { Batch, MailStore, ThreadRow } from "./store";

function without(thread: ThreadRow, leaving: ReadonlySet<string>): ThreadRow {
  return {
    ...thread,
    emailIds: thread.emailIds.filter((id) => !leaving.has(id)),
    members: Object.fromEntries(Object.entries(thread.members).filter(([id]) => !leaving.has(id))),
  };
}

// The rows once the server destroyed emails: their headers and bodies
// leave, their threads lose them and leave with the last one, the lists
// drop their rows and the changes still logged for them leave the log.
export async function goneOf(
  store: MailStore,
  accountId: string,
  ids: readonly string[],
): Promise<{ batch: Batch; changes: AppliedChanges }> {
  const leaving = new Set(ids);
  const headers = [...(await store.emails(accountId, ids)).values()];
  const threadIds = [...new Set(headers.map((header) => header.threadId))];
  const threads = [...(await store.threads(accountId, threadIds)).values()].map((thread) =>
    without(thread, leaving),
  );
  const lists = (await store.queries(accountId)).filter((row) => strip(row, leaving));
  const logged = (await store.pending(accountId)).filter((row) => leaving.has(row.id));
  return {
    batch: {
      emails: { remove: ids },
      bodies: { remove: ids },
      threads: {
        put: threads.filter((thread) => thread.emailIds.length > 0),
        remove: threads.filter((thread) => thread.emailIds.length === 0).map(({ id }) => id),
      },
      queries: { put: lists },
      pending: { remove: logged.map((row) => row.seq) },
    },
    changes: { mailboxes: false, windows: lists.map(({ id }) => id), threads: threadIds },
  };
}
