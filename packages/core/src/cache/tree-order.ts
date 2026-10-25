// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Mailbox } from "../jmap/schemas";

// The special-use roles in the order the tree draws them.
const ROLE_ORDER = ["inbox", "drafts", "sent", "archive", "junk", "trash"] as const;

// The mailbox ids in the order the tree draws them: the roles first,
// then each folder with its children right after it. A folder under a
// role or under a parent the account lacks stands at the top.
export function treeOrder(mailboxes: readonly Mailbox[], locale: string): string[] {
  const collator = new Intl.Collator(locale);
  const sorted = mailboxes.toSorted(
    (first, second) =>
      first.sortOrder - second.sortOrder || collator.compare(first.name, second.name),
  );
  const roles = ROLE_ORDER.flatMap((role) => sorted.find((row) => row.role === role) ?? []);
  const roleIds = new Set(roles.map((row) => row.id));
  const folders = sorted.filter((row) => !roleIds.has(row.id));
  const folderIds = new Set(folders.map((row) => row.id));
  const children = Map.groupBy(folders, (row) =>
    row.parentId !== null && folderIds.has(row.parentId) ? row.parentId : null,
  );
  const order = roles.map((row) => row.id);
  const walk = (parentId: string | null): void => {
    for (const row of children.get(parentId) ?? []) {
      order.push(row.id);
      walk(row.id);
    }
  };
  walk(null);
  return order;
}

// The place of every mailbox in the tree, the inbox first, for the
// mailbox a search result acts from.
export type TreePlaces = ReadonlyMap<string, number>;

export function treePlaces(mailboxes: readonly Mailbox[], locale: string): TreePlaces {
  return new Map(treeOrder(mailboxes, locale).map((id, place) => [id, place]));
}
