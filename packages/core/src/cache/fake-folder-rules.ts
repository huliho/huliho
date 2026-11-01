// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// What a Mailbox/set object must hold against the fake's mailboxes
// before it is written (RFC 8621 section 2.5): a name that fits beside
// its siblings, a parent that takes a child, a role of the six that
// INBOX keeps, a subscription a role mailbox keeps and the right each
// change needs.

import type { Mailbox } from "../jmap/schemas";
import type { FakeJmap } from "./fake-jmap";
import type { Args } from "./fake-wire";

export interface SetError {
  type: string;
  properties?: string[];
}

// One create of the call: its creation id, what it sent and the row.
export interface Create {
  key: string;
  sent: Args;
  row: Mailbox;
}

// The six roles a client gives, each with the place the tree gives it; a
// mailbox without one comes after them, as `mailbox` places it.
export const ROLE_ORDER: ReadonlyMap<string, number> = new Map([
  ["inbox", 0],
  ["drafts", 1],
  ["sent", 2],
  ["archive", 3],
  ["junk", 4],
  ["trash", 5],
]);
export const FOLDER_ORDER = 10;

const encoder = new TextEncoder();

export function refusal(type: string, properties?: string[]): SetError {
  return properties === undefined ? { type } : { type, properties };
}

// The rows standing and the ones the call's creates hold.
export function rowsWith(server: FakeJmap, made: ReadonlyMap<string, Create>): Mailbox[] {
  return [...server.mailboxes.values(), ...[...made.values()].map((create) => create.row)];
}

// Whether a name fits: not empty, no white space at either end and no
// longer than the session's limit.
function fits(server: FakeJmap, name: unknown): name is string {
  const limit = server.maxSizeMailboxName ?? Number.POSITIVE_INFINITY;
  return (
    typeof name === "string" &&
    name !== "" &&
    name.trim() === name &&
    encoder.encode(name).length <= limit
  );
}

function taken(
  rows: readonly Mailbox[],
  name: string,
  parentId: string | null,
  self: string,
): boolean {
  return rows.some((row) => row.id !== self && row.name === name && row.parentId === parentId);
}

// Whether the mailbox is the other one or lies under it.
function under(rows: readonly Mailbox[], id: string, ancestor: string): boolean {
  const byId = new Map(rows.map((row) => [row.id, row]));
  for (let at = byId.get(id); at !== undefined; at = byId.get(at.parentId ?? "")) {
    if (at.id === ancestor) {
      return true;
    }
  }
  return false;
}

// The parent a value names: null for the top level, a row or a create of
// the call by its creation id, undefined for anything else.
export function parentOf(
  server: FakeJmap,
  value: unknown,
  made: ReadonlyMap<string, Create>,
): Mailbox | null | undefined {
  if (value === null) {
    return null;
  }
  if (typeof value !== "string") {
    return undefined;
  }
  return value.startsWith("#") ? made.get(value.slice(1))?.row : server.mailboxes.get(value);
}

// A parent the rows or the call hold that takes a child; the top level
// by the session's word.
function parentError(server: FakeJmap, parent: Mailbox | null | undefined): SetError | null {
  if (parent === undefined) {
    return refusal("invalidProperties", ["parentId"]);
  }
  const takes = parent === null ? server.topLevel : parent.myRights.mayCreateChild;
  return takes ? null : refusal("forbidden");
}

// A role is none or one of the six; INBOX keeps `inbox` and no other
// mailbox takes it.
function roleError(row: Mailbox | null, object: Args): SetError | null {
  if (!("role" in object)) {
    return null;
  }
  const { role } = object;
  const known = role === null || (typeof role === "string" && ROLE_ORDER.has(role));
  const fitting = row?.role === "inbox" ? role === "inbox" : role !== "inbox";
  return known && fitting ? null : refusal("invalidProperties", ["role"]);
}

// A mailbox that keeps a role stays shown.
function subscriptionError(row: Mailbox | null, object: Args): SetError | null {
  const role = "role" in object ? object["role"] : (row?.role ?? null);
  return object["isSubscribed"] === false && role !== null ? refusal("forbidden") : null;
}

export function createError(
  server: FakeJmap,
  object: Args,
  made: ReadonlyMap<string, Create>,
): SetError | null {
  const { name } = object;
  if (!fits(server, name)) {
    return refusal("invalidProperties", ["name"]);
  }
  const parent = parentOf(server, object["parentId"] ?? null, made);
  const fails = parentError(server, parent);
  if (fails !== null) {
    return fails;
  }
  if (taken(rowsWith(server, made), name, parent?.id ?? null, "")) {
    return refusal("invalidProperties", ["name"]);
  }
  return roleError(null, object) ?? subscriptionError(null, object);
}

// A new parent: one the rows or the call hold that takes a child and
// lies outside the mailbox itself.
function moveError(
  server: FakeJmap,
  row: Mailbox,
  patch: Args,
  made: ReadonlyMap<string, Create>,
): SetError | null {
  if (!("parentId" in patch)) {
    return null;
  }
  const parent = parentOf(server, patch["parentId"], made);
  const inside = parent?.id !== undefined && under(rowsWith(server, made), parent.id, row.id);
  return inside ? refusal("invalidProperties", ["parentId"]) : parentError(server, parent);
}

// The parent the mailbox ends under once the patch is in place.
function parentAfter(
  server: FakeJmap,
  row: Mailbox,
  patch: Args,
  made: ReadonlyMap<string, Create>,
): string | null {
  return "parentId" in patch
    ? (parentOf(server, patch["parentId"], made)?.id ?? null)
    : row.parentId;
}

// A new name or parent: never for INBOX and only with the right to
// rename; the name fits, the parent takes it and no sibling under it
// holds the name.
function renameError(
  server: FakeJmap,
  row: Mailbox,
  patch: Args,
  made: ReadonlyMap<string, Create>,
): SetError | null {
  if (!("name" in patch) && !("parentId" in patch)) {
    return null;
  }
  if (row.role === "inbox" || !row.myRights.mayRename) {
    return refusal("forbidden");
  }
  const name = "name" in patch ? patch["name"] : row.name;
  if (!fits(server, name)) {
    return refusal("invalidProperties", ["name"]);
  }
  const moved = moveError(server, row, patch, made);
  if (moved !== null) {
    return moved;
  }
  return taken(rowsWith(server, made), name, parentAfter(server, row, patch, made), row.id)
    ? refusal("invalidProperties", ["name"])
    : null;
}

export function updateError(
  server: FakeJmap,
  row: Mailbox,
  patch: Args,
  made: ReadonlyMap<string, Create>,
): SetError | null {
  return (
    renameError(server, row, patch, made) ?? roleError(row, patch) ?? subscriptionError(row, patch)
  );
}

// A destroy: never of a role mailbox, only with the right to delete and
// never while a mailbox lies under it; mail goes with it only under the
// argument.
export function destroyError(server: FakeJmap, id: string, removeEmails: boolean): SetError | null {
  const row = server.mailboxes.get(id);
  if (row === undefined) {
    return refusal("notFound");
  }
  if (row.role !== null || !row.myRights.mayDelete) {
    return refusal("forbidden");
  }
  if ([...server.mailboxes.values()].some((other) => other.parentId === id)) {
    return refusal("mailboxHasChild");
  }
  const holds = [...server.emails.values()].some((email) => id in email.mailboxIds);
  return holds && !removeEmails ? refusal("mailboxHasEmail") : null;
}
