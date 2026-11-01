// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { callsFor, outcome } from "../jmap/calls";
import type { Outcome } from "../jmap/calls";
import { JmapError, MethodFailure, SERVER_UNAVAILABLE } from "../jmap/client";
import type { JmapClient } from "../jmap/client";
import { mailboxSetAnswerSchema } from "../jmap/schemas";
import type { Invocation, Mailbox, MailboxSetAnswer } from "../jmap/schemas";
import { syncMailboxes } from "./mailboxes";
import { applyChanges } from "./poll";
import type { AppliedChanges } from "./poll";
import type { MailStore } from "./store";

// The call id of the write inside the request it leads.
const SET_CALL = "s";

// A folder as a create names it (RFC 8621 section 2); a parent may be a
// create of the same write, named by `#` and its creation id.
export type MailboxCreate = {
  name: string;
  parentId: string | null;
  role?: string | null;
  isSubscribed?: boolean;
};

// The properties of a folder a write changes.
export type MailboxUpdate = Partial<Pick<Mailbox, "name" | "parentId" | "role" | "isSubscribed">>;

// One write of the account's folders: the creates by creation id, the
// updates by id and the ids to destroy, with whether a destroy takes
// the folder's emails along (RFC 8621 section 2.5).
export interface MailboxWrite {
  create?: Record<string, MailboxCreate>;
  update?: Record<string, MailboxUpdate>;
  destroy?: readonly string[];
  onDestroyRemoveEmails?: boolean;
}

// Why the server did not write one object: the SetError's type and the
// properties an `invalidProperties` names (RFC 8620 section 5.3). The
// server's own description is never kept.
export interface MailboxRefusal {
  type: string;
  properties: string[];
}

// What a write did: the new id of each create by its creation id, the
// ids updated and destroyed, every object refused with its reason and
// what the tabs read again.
export interface MailboxSetOutcome {
  created: Record<string, string>;
  updated: string[];
  destroyed: string[];
  notCreated: Record<string, MailboxRefusal>;
  notUpdated: Record<string, MailboxRefusal>;
  notDestroyed: Record<string, MailboxRefusal>;
  changes: AppliedChanges;
}

// An object the answer names nowhere, read as a server failure.
const UNANSWERED: MailboxRefusal = { type: "serverFail", properties: [] };

type SetErrors = MailboxSetAnswer["notUpdated"];

function objectsOf(write: MailboxWrite): number {
  return (
    Object.keys(write.create ?? {}).length +
    Object.keys(write.update ?? {}).length +
    (write.destroy?.length ?? 0)
  );
}

// The objects asked that the server did not write, each with the
// SetError it named.
function refusalsOf(
  asked: readonly string[],
  written: (key: string) => boolean,
  errors: SetErrors,
): Record<string, MailboxRefusal> {
  const named = new Map(Object.entries(errors ?? {}));
  return Object.fromEntries(
    asked
      .filter((key) => !written(key))
      .map((key): [string, MailboxRefusal] => {
        const error = named.get(key);
        return [
          key,
          error === undefined
            ? UNANSWERED
            : { type: error.type, properties: error.properties ?? [] },
        ];
      }),
  );
}

// The write as the answer settled it; a call refused whole throws.
function settled(
  write: MailboxWrite,
  read: Outcome<MailboxSetAnswer>,
): Omit<MailboxSetOutcome, "changes"> {
  if (read.status === "error") {
    if (read.type === SERVER_UNAVAILABLE) {
      throw new JmapError("unavailable");
    }
    throw new MethodFailure(read.type, SET_CALL);
  }
  const answer = read.value;
  const made = new Map(Object.entries(answer.created ?? {}));
  const updated = new Set(Object.keys(answer.updated ?? {}));
  const destroyed = new Set(answer.destroyed ?? []);
  const creates = Object.keys(write.create ?? {});
  const updates = Object.keys(write.update ?? {});
  const destroys = write.destroy ?? [];
  return {
    created: Object.fromEntries(
      creates.flatMap((key) => {
        const row = made.get(key);
        return row === undefined ? [] : [[key, row.id]];
      }),
    ),
    updated: updates.filter((id) => updated.has(id)),
    destroyed: destroys.filter((id) => destroyed.has(id)),
    notCreated: refusalsOf(creates, (key) => made.has(key), answer.notCreated),
    notUpdated: refusalsOf(updates, (id) => updated.has(id), answer.notUpdated),
    notDestroyed: refusalsOf(destroys, (id) => destroyed.has(id), answer.notDestroyed),
  };
}

// One write of the account's folders, run under the account's lock: one
// Mailbox/set in front of a poll's request. What the server wrote lands
// with every email and list it took along before the call resolves; a
// write past `maxObjectsInSet` is never sent.
export async function setMailbox(
  client: JmapClient,
  store: MailStore,
  write: MailboxWrite,
  watched: readonly string[],
): Promise<MailboxSetOutcome> {
  const session = await client.session();
  if (objectsOf(write) > session.maxObjectsInSet) {
    throw new JmapError("limit", { limit: "maxObjectsInSet" });
  }
  if ((await store.state(client.accountId, "Mailbox")) === null) {
    await syncMailboxes(client, store);
  }
  const set = callsFor(session.accountId).set("Mailbox", { ...write, ifInState: null }, SET_CALL);
  const led: (readonly Invocation[])[] = [];
  const changes = await applyChanges(client, store, watched, {
    calls: [set],
    read: (responses) => {
      led.push(responses);
    },
  });
  const read = outcome(led.at(0) ?? [], SET_CALL, mailboxSetAnswerSchema);
  return { ...settled(write, read), changes };
}
