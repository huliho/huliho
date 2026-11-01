// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// Mailbox/set over the fake's mailboxes, as RFC 8621 section 2.5 has it:
// a create under a parent or at the top with a new id, an update of the
// name, the parent, the role or the subscription and a destroy that takes
// the emails whose only mailbox it was. Every object is checked before
// anything changes and a role against the rows the whole call leaves.

import type { Mailbox } from "../jmap/schemas";
import { z } from "../schema";
import {
  FOLDER_ORDER,
  ROLE_ORDER,
  createError,
  destroyError,
  parentOf,
  refusal,
  rowsWith,
  updateError,
} from "./fake-folder-rules";
import type { Create, SetError } from "./fake-folder-rules";
import type { FakeJmap } from "./fake-jmap";
import { MethodError, UPSTREAM } from "./fake-wire";
import type { Args } from "./fake-wire";
import { recountMailboxes, setArgsOf } from "./fake-writes";

const objectsSchema = z.nullish(z.record(z.string(), z.record(z.string(), z.unknown())));

const mailboxSetArgsSchema = z.object({
  accountId: z.string(),
  ifInState: z.nullish(z.string()),
  create: objectsSchema,
  update: objectsSchema,
  destroy: z.nullish(z.array(z.string())),
  onDestroyRemoveEmails: z.optional(z.boolean()),
});

type SetArgs = z.infer<typeof mailboxSetArgsSchema>;

// A mailbox the user creates on a server without access control lists
// holds every right but submission.
const CREATED_RIGHTS: Mailbox["myRights"] = {
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

// One update of the call: the id it was named by, the row and the patch.
interface Update {
  key: string;
  row: Mailbox;
  patch: Args;
}

// What a call answers, filled as it runs.
interface Answer {
  created: Map<string, Args>;
  updated: Map<string, Args | null>;
  destroyed: string[];
  notCreated: Map<string, SetError>;
  notUpdated: Map<string, SetError>;
  notDestroyed: Map<string, SetError>;
}

function placeOf(role: string | null): number {
  return ROLE_ORDER.get(role ?? "") ?? FOLDER_ORDER;
}

function newRow(server: FakeJmap, object: Args, made: ReadonlyMap<string, Create>): Mailbox {
  server.nextMailbox += 1;
  const role = typeof object["role"] === "string" ? object["role"] : null;
  return {
    id: `mb-new-${String(server.nextMailbox)}`,
    name: String(object["name"]),
    parentId: parentOf(server, object["parentId"] ?? null, made)?.id ?? null,
    role,
    sortOrder: placeOf(role),
    totalEmails: 0,
    unreadEmails: 0,
    totalThreads: 0,
    unreadThreads: 0,
    myRights: { ...CREATED_RIGHTS },
    isSubscribed: object["isSubscribed"] !== false,
    syncedEmails: 0,
  };
}

// The creates of the call, each parent before the creates that name it
// by creation id (RFC 8620 section 5.3); one whose parent never resolves
// fails on it.
function planCreates(server: FakeJmap, args: SetArgs, answer: Answer): Map<string, Create> {
  const made = new Map<string, Create>();
  let pending = Object.entries(args.create ?? {});
  const waits = ([, object]: [string, Args]): boolean => {
    const parentId = object["parentId"];
    return typeof parentId === "string" && parentId.startsWith("#") && !made.has(parentId.slice(1));
  };
  for (let round = pending.length; round > 0 && pending.length > 0; round -= 1) {
    for (const [key, object] of pending.filter((entry) => !waits(entry))) {
      const error = createError(server, object, made);
      if (error === null) {
        made.set(key, { key, sent: object, row: newRow(server, object, made) });
      } else {
        answer.notCreated.set(key, error);
      }
    }
    pending = pending.filter(([key]) => !made.has(key) && !answer.notCreated.has(key));
  }
  for (const [key] of pending) {
    answer.notCreated.set(key, refusal("invalidProperties", ["parentId"]));
  }
  return made;
}

function planUpdates(
  server: FakeJmap,
  args: SetArgs,
  made: ReadonlyMap<string, Create>,
  answer: Answer,
): Update[] {
  const planned: Update[] = [];
  for (const [key, patch] of Object.entries(args.update ?? {})) {
    const row = server.mailboxes.get(key);
    if (row === undefined) {
      answer.notUpdated.set(key, refusal("notFound"));
      continue;
    }
    const error = updateError(server, row, patch, made);
    if (error === null) {
      planned.push({ key, row, patch });
    } else {
      answer.notUpdated.set(key, error);
    }
  }
  return planned;
}

// Refuses every create and update whose role would stand on two
// mailboxes once the whole call is in place (RFC 8621 section 2); the
// updates that stand come back.
function settleRoles(
  server: FakeJmap,
  made: Map<string, Create>,
  updates: readonly Update[],
  answer: Answer,
): Update[] {
  const finalRole = (row: Mailbox): unknown => {
    const update = updates.find((one) => one.row === row);
    return update !== undefined && "role" in update.patch ? update.patch["role"] : row.role;
  };
  const holders = new Map<unknown, number>();
  for (const row of rowsWith(server, made)) {
    const role = finalRole(row);
    holders.set(role, (holders.get(role) ?? 0) + 1);
  }
  const doubled = (role: unknown): boolean => role !== null && (holders.get(role) ?? 0) > 1;
  for (const [key, create] of made) {
    if (doubled(create.row.role)) {
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

// The properties of the row the client did not send or that moved
// beyond what it sent, as RFC 8620 section 5.3 answers them.
function beyond(row: Mailbox, sent: Args, before: Mailbox | null): Args | null {
  const told = new Map(Object.entries(sent));
  const held = new Map<string, unknown>(Object.entries(before ?? {}));
  const changed = Object.entries(row).filter(([name, value]) => {
    if (told.has(name)) {
      return JSON.stringify(told.get(name)) !== JSON.stringify(value);
    }
    return before === null || JSON.stringify(held.get(name)) !== JSON.stringify(value);
  });
  return changed.length === 0 ? null : Object.fromEntries(changed);
}

function applyCreate(server: FakeJmap, create: Create, answer: Answer): void {
  server.putMailbox(create.row);
  answer.created.set(create.key, beyond(create.row, create.sent, null) ?? { id: create.row.id });
}

function applyUpdate(
  server: FakeJmap,
  update: Update,
  made: ReadonlyMap<string, Create>,
  answer: Answer,
): void {
  const { row, patch } = update;
  const after: Mailbox = { ...row };
  if (typeof patch["name"] === "string") {
    after.name = patch["name"];
  }
  if ("parentId" in patch) {
    after.parentId = parentOf(server, patch["parentId"], made)?.id ?? null;
  }
  if ("role" in patch) {
    after.role = typeof patch["role"] === "string" ? patch["role"] : null;
    after.sortOrder = placeOf(after.role);
  }
  if (typeof patch["isSubscribed"] === "boolean") {
    after.isSubscribed = patch["isSubscribed"];
  }
  server.putMailbox(after);
  answer.updated.set(row.id, beyond(after, patch, row));
}

// The emails whose only mailbox it was leave with it; the rest leave it.
function applyDestroy(server: FakeJmap, id: string, answer: Answer): void {
  const inside = [...server.emails.values()].filter((email) => id in email.mailboxIds);
  for (const email of inside) {
    const kept = Object.entries(email.mailboxIds).filter(([box]) => box !== id);
    if (kept.length === 0) {
      server.destroyEmail(email.id);
    } else {
      server.amend(email.id, { mailboxIds: Object.fromEntries(kept) });
    }
  }
  server.removeMailbox(id);
  const touched = new Set(inside.flatMap((email) => Object.keys(email.mailboxIds)));
  recountMailboxes(
    server,
    [...touched].filter((box) => box !== id),
  );
  answer.destroyed.push(id);
}

function destroyAll(server: FakeJmap, args: SetArgs, answer: Answer): void {
  for (const id of args.destroy ?? []) {
    const error = destroyError(server, id, args.onDestroyRemoveEmails === true);
    if (error === null) {
      applyDestroy(server, id, answer);
    } else {
      answer.notDestroyed.set(id, error);
    }
  }
}

// The scripted refusal: every object of the call answers it and nothing
// changes.
function refuseAll(args: SetArgs, type: string, answer: Answer): void {
  for (const key of Object.keys(args.create ?? {})) {
    answer.notCreated.set(key, refusal(type));
  }
  for (const key of Object.keys(args.update ?? {})) {
    answer.notUpdated.set(key, refusal(type));
  }
  for (const id of args.destroy ?? []) {
    answer.notDestroyed.set(id, refusal(type));
  }
}

function named<Value>(map: ReadonlyMap<string, Value>): Record<string, Value> | null {
  return map.size === 0 ? null : Object.fromEntries(map);
}

function rendered(server: FakeJmap, oldState: string, answer: Answer): Args {
  return {
    accountId: UPSTREAM,
    oldState,
    newState: String(server.sequence),
    created: named(answer.created),
    updated: named(answer.updated),
    destroyed: answer.destroyed.length === 0 ? null : answer.destroyed,
    notCreated: named(answer.notCreated),
    notUpdated: named(answer.notUpdated),
    notDestroyed: named(answer.notDestroyed),
  };
}

export function mailboxSet(server: FakeJmap, raw: Args): Args {
  const args = setArgsOf(server, mailboxSetArgsSchema, raw);
  const count =
    Object.keys(args.create ?? {}).length +
    Object.keys(args.update ?? {}).length +
    (args.destroy?.length ?? 0);
  if (count > server.maxObjectsInSet) {
    throw new MethodError("requestTooLarge");
  }
  const oldState = String(server.sequence);
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
