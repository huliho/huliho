// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// What a Mailbox/set object must hold against the mailbox rows before
// the mock writes it (RFC 8621 section 2.5): a name that fits beside its
// siblings, a parent the rows or the call hold that takes a child, a
// role of the six that INBOX keeps, a subscription a role mailbox keeps
// and the rights each change needs.

import type { MailServer } from "./mail-answers";

export type Row = Record<string, unknown>;

export interface SetError {
  type: string;
  properties?: string[];
}

// One create the call makes: its creation id, what it sent and the row.
export interface Create {
  key: string;
  sent: Row;
  row: Row;
}

// The properties a client sets (RFC 8621 section 2).
const SETTABLE = new Set(["name", "parentId", "role", "isSubscribed"]);

// The six roles the client offers, each with the place the tree gives
// it; a mailbox without a role comes after them.
export const ROLE_ORDER = new Map([
  ["inbox", 0],
  ["drafts", 1],
  ["sent", 2],
  ["archive", 3],
  ["junk", 4],
  ["trash", 5],
]);
export const FOLDER_ORDER = 10;

// The longest name in octets of UTF-8, as the session's mail limits say.
export const MAX_NAME_BYTES = 255;

const encoder = new TextEncoder();

export function isRecord(value: unknown): value is Row {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function refusal(type: string, properties?: string[]): SetError {
  return properties === undefined ? { type } : { type, properties };
}

export function rowById(rows: readonly Row[], id: unknown): Row | undefined {
  return rows.find((row) => row["id"] === id);
}

// The rows standing and the ones the call's creates hold.
export function rowsWith(server: MailServer, made: Map<string, Create>): Row[] {
  return [...server.mailboxes, ...[...made.values()].map((create) => create.row)];
}

function may(row: Row, right: string): boolean {
  const rights = row["myRights"];
  return isRecord(rights) && new Map(Object.entries(rights)).get(right) === true;
}

// Whether a name fits: not empty, no white space at either end and at
// most MAX_NAME_BYTES of UTF-8.
function fits(name: unknown): name is string {
  return (
    typeof name === "string" &&
    name !== "" &&
    name.trim() === name &&
    encoder.encode(name).length <= MAX_NAME_BYTES
  );
}

// Whether another mailbox under the parent holds the name.
function taken(rows: readonly Row[], name: string, parentId: unknown, self: unknown): boolean {
  return rows.some(
    (row) => row["id"] !== self && row["name"] === name && row["parentId"] === parentId,
  );
}

// Whether the mailbox is the other one or lies under it.
function under(rows: readonly Row[], id: unknown, ancestor: unknown): boolean {
  let at = rowById(rows, id);
  while (at !== undefined) {
    if (at["id"] === ancestor) {
      return true;
    }
    at = rowById(rows, at["parentId"]);
  }
  return false;
}

// The parent a value names: null for the top level, a row or a create of
// the call by its creation id, undefined for anything else.
export function parentOf(
  server: MailServer,
  value: unknown,
  made: Map<string, Create>,
): Row | null | undefined {
  if (value === null) {
    return null;
  }
  if (typeof value !== "string") {
    return undefined;
  }
  return value.startsWith("#") ? made.get(value.slice(1))?.row : rowById(server.mailboxes, value);
}

// What a parent asks of a mailbox it takes: one the rows or the call
// hold, the right to take a child and, for the top level, the session's
// word.
function parentError(server: MailServer, parent: Row | null | undefined): SetError | null {
  if (parent === undefined) {
    return refusal("invalidProperties", ["parentId"]);
  }
  if (parent === null) {
    return server.topLevel ? null : refusal("forbidden");
  }
  return may(parent, "mayCreateChild") ? null : refusal("forbidden");
}

function unsettable(object: Row): SetError | null {
  const bad = Object.keys(object).filter((key) => !SETTABLE.has(key));
  return bad.length === 0 ? null : refusal("invalidProperties", bad);
}

// A role is none or one of the six; INBOX keeps `inbox` and no other
// mailbox takes it.
function roleError(row: Row | null, object: Row): SetError | null {
  if (!("role" in object)) {
    return null;
  }
  const role = object["role"];
  const inbox = row?.["role"] === "inbox";
  const known = role === null || (typeof role === "string" && ROLE_ORDER.has(role));
  const fitting = inbox ? role === "inbox" : role !== "inbox";
  return known && fitting ? null : refusal("invalidProperties", ["role"]);
}

// A subscription is a boolean; a mailbox that keeps a role stays shown.
function subscriptionError(row: Row | null, object: Row): SetError | null {
  if (!("isSubscribed" in object)) {
    return null;
  }
  const subscribed = object["isSubscribed"];
  if (typeof subscribed !== "boolean") {
    return refusal("invalidProperties", ["isSubscribed"]);
  }
  const role = "role" in object ? object["role"] : (row?.["role"] ?? null);
  return !subscribed && role !== null ? refusal("forbidden") : null;
}

export function createError(
  server: MailServer,
  object: Row,
  made: Map<string, Create>,
): SetError | null {
  const bad = unsettable(object);
  if (bad !== null) {
    return bad;
  }
  const name = object["name"];
  if (!fits(name)) {
    return refusal("invalidProperties", ["name"]);
  }
  const parent = parentOf(server, object["parentId"] ?? null, made);
  const fails = parentError(server, parent);
  if (fails !== null) {
    return fails;
  }
  if (taken(rowsWith(server, made), name, parent?.["id"] ?? null, null)) {
    return refusal("invalidProperties", ["name"]);
  }
  return roleError(null, object) ?? subscriptionError(null, object);
}

// A new parent: one the rows or the call hold that takes a child and
// lies outside the mailbox itself.
function moveError(
  server: MailServer,
  row: Row,
  patch: Row,
  made: Map<string, Create>,
): SetError | null {
  if (!("parentId" in patch)) {
    return null;
  }
  const parent = parentOf(server, patch["parentId"], made);
  const inside = isRecord(parent) && under(rowsWith(server, made), parent["id"], row["id"]);
  return inside ? refusal("invalidProperties", ["parentId"]) : parentError(server, parent);
}

// The name and the parent id the mailbox ends with under the patch.
function target(
  server: MailServer,
  row: Row,
  patch: Row,
  made: Map<string, Create>,
): [unknown, unknown] {
  const name = "name" in patch ? patch["name"] : row["name"];
  if (!("parentId" in patch)) {
    return [name, row["parentId"]];
  }
  return [name, parentOf(server, patch["parentId"], made)?.["id"] ?? null];
}

// A new name or parent: never for INBOX and only with the right to
// rename; the name fits and no sibling under the parent holds it.
function renameError(
  server: MailServer,
  row: Row,
  patch: Row,
  made: Map<string, Create>,
): SetError | null {
  if (!("name" in patch) && !("parentId" in patch)) {
    return null;
  }
  if (row["role"] === "inbox" || !may(row, "mayRename")) {
    return refusal("forbidden");
  }
  const [name, parentId] = target(server, row, patch, made);
  if (!fits(name)) {
    return refusal("invalidProperties", ["name"]);
  }
  const moved = moveError(server, row, patch, made);
  if (moved !== null) {
    return moved;
  }
  const clash = taken(rowsWith(server, made), name, parentId, row["id"]);
  return clash ? refusal("invalidProperties", ["name"]) : null;
}

export function updateError(
  server: MailServer,
  row: Row,
  patch: unknown,
  made: Map<string, Create>,
): SetError | null {
  if (!isRecord(patch)) {
    return refusal("invalidProperties");
  }
  return (
    unsettable(patch) ??
    renameError(server, row, patch, made) ??
    roleError(row, patch) ??
    subscriptionError(row, patch)
  );
}

// A destroy: never of a role mailbox, only with the right to delete and
// never while a mailbox lies under it; mail goes with it only under the
// argument.
export function destroyError(
  server: MailServer,
  id: unknown,
  removeEmails: boolean,
): SetError | null {
  const row = rowById(server.mailboxes, id);
  if (row === undefined) {
    return refusal("notFound");
  }
  if (row["role"] !== null || !may(row, "mayDelete")) {
    return refusal("forbidden");
  }
  if (server.mailboxes.some((other) => other["parentId"] === id)) {
    return refusal("mailboxHasChild");
  }
  const held = server.corpus.lists.get(String(id))?.ids.length ?? 0;
  return held > 0 && !removeEmails ? refusal("mailboxHasEmail") : null;
}
