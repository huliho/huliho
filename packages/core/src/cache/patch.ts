// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailPatch, MemberState, PendingRow } from "./store";

// The two sets of an email a patch names members of (RFC 8621 section
// 4.1.1).
type Members = keyof MemberState;

const SETS: readonly Members[] = ["keywords", "mailboxIds"];

// The path of one member of a set; a pointer token writes "~" as ~0 and
// "/" as ~1 (RFC 6901 section 3).
export function pathOf(set: Members, key: string): string {
  return `${set}/${key.replaceAll("~", "~0").replaceAll("/", "~1")}`;
}

// The set and the member a path names, its escapes undone in the order
// RFC 6901 section 4 gives.
export function targetOf(path: string): { set: Members; key: string } {
  const set = SETS.find((name) => path.startsWith(`${name}/`) && path.length > name.length + 1);
  if (set === undefined) {
    throw new Error("a patch names keywords or mailboxes alone");
  }
  return {
    set,
    key: path
      .slice(set.length + 1)
      .replaceAll("~1", "/")
      .replaceAll("~0", "~"),
  };
}

// The state with a patch laid over it and the patch that takes it back:
// each path set to what the state held before.
export function patched(
  state: MemberState,
  patch: EmailPatch,
): { state: MemberState; inverse: EmailPatch } {
  const keywords = new Map(Object.entries(state.keywords));
  const mailboxIds = new Map(Object.entries(state.mailboxIds));
  const inverse: [string, true | null][] = [];
  for (const [path, value] of Object.entries(patch)) {
    const { set, key } = targetOf(path);
    const members = set === "keywords" ? keywords : mailboxIds;
    inverse.push([path, members.has(key) ? true : null]);
    if (value === null) {
      members.delete(key);
    } else {
      members.set(key, true);
    }
  }
  return {
    state: { keywords: Object.fromEntries(keywords), mailboxIds: Object.fromEntries(mailboxIds) },
    inverse: Object.fromEntries(inverse),
  };
}

// The patch that takes an email out of every mailbox it stands in.
export function emptied(state: MemberState): EmailPatch {
  return Object.fromEntries(
    Object.keys(state.mailboxIds).map((id) => [pathOf("mailboxIds", id), null]),
  );
}

// Whether a patch changes anything, given the patch that takes it back.
export function alters(patch: EmailPatch, inverse: EmailPatch): boolean {
  const before = new Map(Object.entries(inverse));
  return Object.entries(patch).some(([path, value]) => before.get(path) !== value);
}

// The state with every pending row of the email laid over it, the
// oldest first; a destroy empties the mailboxes the email stands in
// then. Each row comes back with the inverse that fits this state.
export function replayed(
  state: MemberState,
  rows: readonly PendingRow[],
): { state: MemberState; rows: PendingRow[] } {
  let current = state;
  const fitted = rows.map((row) => {
    const patch = row.destroy === true ? emptied(current) : row.patch;
    const step = patched(current, patch);
    current = step.state;
    return { ...row, patch, inverse: step.inverse };
  });
  return { state: current, rows: fitted };
}

// The state as it stood before the pending rows, the newest undone
// first.
export function unpatched(state: MemberState, rows: readonly PendingRow[]): MemberState {
  return rows.reduceRight((current, row) => patched(current, row.inverse).state, state);
}
