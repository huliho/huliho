// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { JmapClient } from "../jmap/client";
import type { Mailbox } from "../jmap/schemas";
import type { Mutation } from "./pending";
import type { MailStore } from "./store";
import { SEEN } from "./unread";

const SEEN_PATH = `keywords/${SEEN}`;

// Whether a mailbox takes every path of a patch: $seen asks maySetSeen
// and any other keyword maySetKeywords (RFC 8621 section 2).
function allows(rights: Mailbox["myRights"], paths: readonly string[]): boolean {
  return paths.every((path) => (path === SEEN_PATH ? rights.maySetSeen : rights.maySetKeywords));
}

// Whether the account and every mailbox of the email take a change, so
// a write the server said it refuses is never logged. A mailbox the
// store does not hold is the server's to judge.
export async function mayPatch(
  client: JmapClient,
  store: MailStore,
  mutation: Mutation,
): Promise<boolean> {
  const session = await client.session();
  if (session.readOnly || session.maxObjectsInSet === 0) {
    return false;
  }
  const { accountId } = client;
  const header = (await store.emails(accountId, [mutation.id])).get(mutation.id);
  if (header === undefined) {
    return false;
  }
  const inside = new Set(Object.keys(header.mailboxIds));
  const boxes = (await store.mailboxes(accountId)).filter((box) => inside.has(box.id));
  const paths = Object.keys(mutation.patch);
  return boxes.every((box) => allows(box.myRights, paths));
}
