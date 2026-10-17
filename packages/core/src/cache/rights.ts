// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { JmapClient } from "../jmap/client";
import type { Mailbox } from "../jmap/schemas";
import { SEEN } from "./counts";
import { knownEmails } from "./known";
import { patched, targetOf } from "./patch";
import type { Mutation, PatchMutation } from "./pending";
import type { MailStore, MemberState } from "./store";

type Rights = Mailbox["myRights"];

// Whether a mailbox the email stands in takes one path. $seen asks
// maySetSeen and any other keyword maySetKeywords (RFC 8621 section 2).
// A move asks mayRemoveItems where the email leaves; where it enters,
// the mailbox has to be one the store holds and asks mayAddItems.
function allows(
  path: string,
  value: true | null,
  inside: readonly Rights[],
  boxes: ReadonlyMap<string, Mailbox>,
): boolean {
  const { set, key } = targetOf(path);
  if (set === "keywords") {
    return inside.every((rights) => (key === SEEN ? rights.maySetSeen : rights.maySetKeywords));
  }
  const box = boxes.get(key);
  if (value === true) {
    return box?.myRights.mayAddItems === true;
  }
  return box === undefined || box.myRights.mayRemoveItems;
}

// A patch every mailbox involved takes that leaves the email in at least
// one mailbox (RFC 8621 section 4.1.1).
function patchAllowed(
  state: MemberState,
  mutation: PatchMutation,
  boxes: ReadonlyMap<string, Mailbox>,
): boolean {
  const inside = Object.keys(state.mailboxIds).flatMap((id) => {
    const box = boxes.get(id);
    return box === undefined ? [] : [box.myRights];
  });
  const paths = Object.entries(mutation.patch);
  const after = patched(state, mutation.patch).state;
  return (
    Object.keys(after.mailboxIds).length > 0 &&
    paths.every(([path, value]) => allows(path, value, inside, boxes))
  );
}

// Whether the account and the mailboxes involved take a change, so a
// write the server said it refuses is never logged. A mailbox the email
// stands in that the store does not hold is the server's to judge; a
// destroy asks mayRemoveItems of every mailbox the email stands in.
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
  const state = (await knownEmails(store, accountId, [mutation])).get(mutation.id)?.state;
  if (state === undefined) {
    return false;
  }
  const boxes = new Map((await store.mailboxes(accountId)).map((box) => [box.id, box]));
  if ("destroy" in mutation) {
    return Object.keys(state.mailboxIds).every(
      (id) => boxes.get(id)?.myRights.mayRemoveItems ?? true,
    );
  }
  return patchAllowed(state, mutation, boxes);
}
