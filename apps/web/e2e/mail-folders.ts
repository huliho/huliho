// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// Mailbox/set over the mailbox rows, as RFC 8621 section 2.5 has it: a
// create under a parent or at the top with a new id, an update of the
// name, the parent, the role or the subscription and a destroy that
// takes the emails whose only mailbox it was. Every object is checked
// against the rows before anything changes and a role against the rows
// the whole call leaves; what lands is logged so the next poll sees it.

import type { MailServer } from "./mail-answers";
import { UPSTREAM } from "./mail-corpus";
import {
  FOLDER_ORDER,
  ROLE_ORDER,
  createError,
  destroyError,
  isRecord,
  parentOf,
  refusal,
  rowById,
  rowsWith,
  updateError,
} from "./mail-folder-rules";
import type { Create, Row, SetError } from "./mail-folder-rules";
import { removeEmail } from "./mail-writes";

type Args = Record<string, unknown>;

// A mailbox the user creates on a server without access control lists
// holds every right but submission.
const CREATED_RIGHTS = {
  mayReadItems: true,
  mayAddItems: true,
  mayRemoveItems: true,
  maySetSeen: true,
  maySetKeywords: true,
  mayCreateChild: true,
  mayRename: true,
  mayDelete: true,
  maySubmit: false,
};

// One update the call makes: the id it was named by, the row and the
// patch.
interface Update {
  key: string;
  row: Row;
  patch: Row;
}

// What a call answers, filled as it runs.
interface Answer {
  created: Map<string, Row>;
  updated: Map<string, Row | null>;
  destroyed: string[];
  notCreated: Map<string, SetError>;
  notUpdated: Map<string, SetError>;
  notDestroyed: Map<string, SetError>;
}

function stateOf(server: MailServer): string {
  return String(server.changes.length);
}

function noted(server: MailServer, id: string, kind: "created" | "updated" | "destroyed"): void {
  server.changes.push({ sequence: server.changes.length + 1, type: "Mailbox", id, kind });
}

function newRow(server: MailServer, object: Row, made: Map<string, Create>): Row {
  server.nextMailbox += 1;
  const role = typeof object["role"] === "string" ? object["role"] : null;
  const parent = parentOf(server, object["parentId"] ?? null, made);
  return {
    id: `mb-new-${String(server.nextMailbox)}`,
    name: object["name"],
    parentId: parent?.["id"] ?? null,
    role,
    sortOrder: ROLE_ORDER.get(role ?? "") ?? FOLDER_ORDER,
    totalEmails: 0,
    unreadEmails: 0,
    totalThreads: 0,
    unreadThreads: 0,
    myRights: { ...CREATED_RIGHTS },
    isSubscribed: object["isSubscribed"] !== false,
  };
}

// Decides one create: false while its parent names a create not made
// yet, true once it is planned or refused.
function placed(
  server: MailServer,
  [key, object]: [string, unknown],
  made: Map<string, Create>,
  answer: Answer,
): boolean {
  if (!isRecord(object)) {
    answer.notCreated.set(key, refusal("invalidProperties"));
    return true;
  }
  const parentId = object["parentId"];
  if (typeof parentId === "string" && parentId.startsWith("#") && !made.has(parentId.slice(1))) {
    return false;
  }
  const error = createError(server, object, made);
  if (error === null) {
    made.set(key, { key, sent: object, row: newRow(server, object, made) });
  } else {
    answer.notCreated.set(key, error);
  }
  return true;
}

// The creates of the call, each parent before the creates that name it
// (RFC 8620 section 5.3); one whose parent never resolves fails on it.
function planCreates(server: MailServer, args: Args, answer: Answer): Map<string, Create> {
  const made = new Map<string, Create>();
  let pending = Object.entries(isRecord(args["create"]) ? args["create"] : {});
  for (let round = pending.length; round > 0 && pending.length > 0; round -= 1) {
    pending = pending.filter((entry) => !placed(server, entry, made, answer));
  }
  for (const [key] of pending) {
    answer.notCreated.set(key, refusal("invalidProperties", ["parentId"]));
  }
  return made;
}

function planUpdates(
  server: MailServer,
  args: Args,
  made: Map<string, Create>,
  answer: Answer,
): Update[] {
  const planned: Update[] = [];
  for (const [key, patch] of Object.entries(isRecord(args["update"]) ? args["update"] : {})) {
    const id = key.startsWith("#") ? made.get(key.slice(1))?.row["id"] : key;
    const row = rowById(rowsWith(server, made), id);
    const error = row === undefined ? refusal("notFound") : updateError(server, row, patch, made);
    if (error === null && row !== undefined && isRecord(patch)) {
      planned.push({ key, row, patch });
    } else {
      answer.notUpdated.set(key, error ?? refusal("invalidProperties"));
    }
  }
  return planned;
}

// Refuses every create and update whose role would stand on two
// mailboxes once the whole call is in place (RFC 8621 section 2); the
// updates that stand come back.
function settleRoles(
  server: MailServer,
  made: Map<string, Create>,
  updates: Update[],
  answer: Answer,
): Update[] {
  const finalRole = (row: Row): unknown => {
    const update = updates.find((one) => one.row === row);
    return update !== undefined && "role" in update.patch ? update.patch["role"] : row["role"];
  };
  const holders = new Map<unknown, number>();
  for (const row of rowsWith(server, made)) {
    const role = finalRole(row);
    holders.set(role, (holders.get(role) ?? 0) + 1);
  }
  const doubled = (role: unknown): boolean => role !== null && (holders.get(role) ?? 0) > 1;
  for (const [key, create] of made) {
    if (doubled(create.row["role"])) {
      made.delete(key);
      answer.notCreated.set(key, refusal("invalidProperties", ["role"]));
    }
  }
  return updates.filter((update) => {
    const clashes = "role" in update.patch && doubled(update.patch["role"]);
    if (clashes) {
      answer.notUpdated.set(update.key, refusal("invalidProperties", ["role"]));
    }
    return !clashes;
  });
}

// The properties of the row that differ from what the client sent or
// held before, as RFC 8620 section 5.3 answers them; null for none.
function beyond(row: Row, sent: Row, before: Row | null): Row | null {
  const told = new Map(Object.entries(sent));
  const held = new Map(Object.entries(before ?? {}));
  const changed = Object.entries(row).filter(([name, value]) => {
    if (told.has(name)) {
      return JSON.stringify(told.get(name)) !== JSON.stringify(value);
    }
    return before === null || JSON.stringify(held.get(name)) !== JSON.stringify(value);
  });
  return changed.length === 0 ? null : Object.fromEntries(changed);
}

function applyCreate(server: MailServer, create: Create, answer: Answer): void {
  const id = String(create.row["id"]);
  server.mailboxes.push(create.row);
  server.corpus.lists.set(id, { ids: [], exemplars: [] });
  noted(server, id, "created");
  answer.created.set(create.key, beyond(create.row, create.sent, null) ?? { id });
}

function applyUpdate(
  server: MailServer,
  update: Update,
  made: Map<string, Create>,
  answer: Answer,
): void {
  const { row, patch } = update;
  const before = { ...row };
  const changes: Row = { ...patch };
  if ("parentId" in patch) {
    changes["parentId"] = parentOf(server, patch["parentId"], made)?.["id"] ?? null;
  }
  if ("role" in patch) {
    changes["sortOrder"] = ROLE_ORDER.get(String(patch["role"])) ?? FOLDER_ORDER;
  }
  Object.assign(row, changes);
  const id = String(row["id"]);
  noted(server, id, "updated");
  answer.updated.set(id, beyond(row, patch, before));
}

function applyDestroy(server: MailServer, id: string, answer: Answer): void {
  for (const emailId of server.corpus.lists.get(id)?.ids ?? []) {
    const message = server.corpus.emails.get(emailId);
    if (message !== undefined) {
      removeEmail(server, message);
    }
  }
  server.corpus.lists.delete(id);
  server.mailboxes.splice(
    server.mailboxes.findIndex((row) => row["id"] === id),
    1,
  );
  noted(server, id, "destroyed");
  answer.destroyed.push(id);
}

// The scripted refusal: every object of the call answers it and nothing
// changes.
function refuseAll(args: Args, type: string, answer: Answer): void {
  for (const key of Object.keys(isRecord(args["create"]) ? args["create"] : {})) {
    answer.notCreated.set(key, refusal(type));
  }
  for (const key of Object.keys(isRecord(args["update"]) ? args["update"] : {})) {
    answer.notUpdated.set(key, refusal(type));
  }
  for (const id of Array.isArray(args["destroy"]) ? args["destroy"] : []) {
    answer.notDestroyed.set(String(id), refusal(type));
  }
}

function named<Value>(map: Map<string, Value>): Record<string, Value> | null {
  return map.size === 0 ? null : Object.fromEntries(map);
}

function rendered(server: MailServer, oldState: string, answer: Answer): Args {
  return {
    accountId: UPSTREAM,
    oldState,
    newState: stateOf(server),
    created: named(answer.created),
    updated: named(answer.updated),
    destroyed: answer.destroyed.length === 0 ? null : answer.destroyed,
    notCreated: named(answer.notCreated),
    notUpdated: named(answer.notUpdated),
    notDestroyed: named(answer.notDestroyed),
  };
}

function destroyAll(server: MailServer, args: Args, answer: Answer): void {
  const removeEmails = args["onDestroyRemoveEmails"] === true;
  for (const id of Array.isArray(args["destroy"]) ? args["destroy"] : []) {
    const error = destroyError(server, id, removeEmails);
    if (error === null) {
      applyDestroy(server, String(id), answer);
    } else {
      answer.notDestroyed.set(String(id), error);
    }
  }
}

export function mailboxSet(server: MailServer, args: Args): Args {
  const ifInState = args["ifInState"];
  if (typeof ifInState === "string" && ifInState !== stateOf(server)) {
    throw new Error("stateMismatch");
  }
  const oldState = stateOf(server);
  const answer: Answer = {
    created: new Map(),
    updated: new Map(),
    destroyed: [],
    notCreated: new Map(),
    notUpdated: new Map(),
    notDestroyed: new Map(),
  };
  if (server.folderRefusal !== null) {
    refuseAll(args, server.folderRefusal, answer);
    return rendered(server, oldState, answer);
  }
  const made = planCreates(server, args, answer);
  const updates = settleRoles(server, made, planUpdates(server, args, made, answer), answer);
  for (const create of made.values()) {
    applyCreate(server, create, answer);
  }
  for (const update of updates) {
    applyUpdate(server, update, made, answer);
  }
  destroyAll(server, args, answer);
  return rendered(server, oldState, answer);
}
