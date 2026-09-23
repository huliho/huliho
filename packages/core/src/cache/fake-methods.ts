// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { EmailHeader } from "../jmap/schemas";
import { z } from "../schema";
import type { FakeJmap } from "./fake-jmap";

// The Huliho account id the client is built on, and the upstream id
// the session object names for it.
export const ACCOUNT = "acc-1";
export const UPSTREAM = "u1";

export type Kind = "created" | "updated" | "destroyed";
export type Type = "Mailbox" | "Email" | "Thread";
export type Args = Record<string, unknown>;
export type Invocation = [string, Args, string];

export interface Change {
  sequence: number;
  type: Type;
  id: string;
  kind: Kind;
}

export class MethodError extends Error {
  readonly type: string;

  constructor(type: string) {
    super(type);
    this.type = type;
  }
}

const getArgsSchema = z.object({
  accountId: z.string(),
  ids: z.array(z.string()).nullable().optional(),
  properties: z.array(z.string()).optional(),
});

const comparatorSchema = z.object({ property: z.string(), isAscending: z.boolean().optional() });

const queryArgsSchema = z.object({
  accountId: z.string(),
  filter: z.unknown(),
  sort: z.array(comparatorSchema).optional(),
  position: z.number().int().optional(),
  anchor: z.string().optional(),
  anchorOffset: z.number().int().optional(),
  limit: z.number().int().optional(),
  calculateTotal: z.boolean().optional(),
  collapseThreads: z.boolean().optional(),
});

const inMailboxSchema = z.strictObject({ inMailbox: z.string() });

const changesArgsSchema = z.object({ accountId: z.string(), sinceState: z.string() });

function parse<Value extends { accountId: string }>(schema: z.ZodType<Value>, args: Args): Value {
  const read = schema.safeParse(args);
  if (!read.success) {
    throw new MethodError("invalidArguments");
  }
  if (read.data.accountId !== UPSTREAM) {
    throw new MethodError("accountNotFound");
  }
  return read.data;
}

// The seven methods the cache calls, answered from the fake's maps.
export class FakeMethods {
  private readonly server: FakeJmap;
  private readonly vendor: boolean;

  constructor(server: FakeJmap, vendor: boolean) {
    this.server = server;
    this.vendor = vendor;
  }

  dispatch(name: string, args: Args): Args {
    const methods = new Map<string, (raw: Args) => Args>([
      ["Mailbox/get", (raw) => this.mailboxGet(raw)],
      ["Mailbox/changes", (raw) => this.changes("Mailbox", raw)],
      ["Email/get", (raw) => this.emailGet(raw)],
      ["Email/query", (raw) => this.emailQuery(raw)],
      ["Email/changes", (raw) => this.changes("Email", raw)],
      ["Thread/get", (raw) => this.threadGet(raw)],
      ["Thread/changes", (raw) => this.changes("Thread", raw)],
    ]);
    const method = methods.get(name);
    if (method === undefined) {
      throw new MethodError("unknownMethod");
    }
    return method(args);
  }

  private state(): string {
    return String(this.server.sequence);
  }

  private ids(raw: Args, whole: boolean): { asked: string[] | null; properties: string[] | null } {
    const args = parse(getArgsSchema, raw);
    if (args.ids === null || args.ids === undefined) {
      if (!whole) {
        throw new MethodError("requestTooLarge");
      }
      return { asked: null, properties: args.properties ?? null };
    }
    if (args.ids.length > this.server.maxObjectsInGet) {
      throw new MethodError("requestTooLarge");
    }
    return { asked: [...new Set(args.ids)], properties: args.properties ?? null };
  }

  private mailboxGet(raw: Args): Args {
    const asked = this.ids(raw, true).asked ?? [...this.server.mailboxes.keys()];
    const list = asked.flatMap((id) => {
      const row = this.server.mailboxes.get(id);
      if (row === undefined) {
        return [];
      }
      const { syncedEmails, ...plain } = row;
      return [this.vendor ? { ...plain, syncedEmails } : plain];
    });
    const found = new Set(list.map((row) => row.id));
    return {
      accountId: UPSTREAM,
      state: this.state(),
      list,
      notFound: asked.filter((id) => !found.has(id)),
    };
  }

  private emailGet(raw: Args): Args {
    const { asked, properties } = this.ids(raw, false);
    const wanted = asked ?? [];
    const list = wanted.flatMap((id) => {
      const row = this.server.emails.get(id);
      return row === undefined ? [] : [pick(row, properties)];
    });
    return {
      accountId: UPSTREAM,
      state: this.state(),
      list,
      notFound: wanted.filter((id) => !this.server.emails.has(id)),
    };
  }

  private threadGet(raw: Args): Args {
    const wanted = this.ids(raw, false).asked ?? [];
    const threads = threadsOf(this.server.emails);
    const list = wanted.flatMap((id) => {
      const emailIds = threads.get(id);
      return emailIds === undefined ? [] : [{ id, emailIds }];
    });
    return {
      accountId: UPSTREAM,
      state: this.state(),
      list,
      notFound: wanted.filter((id) => !threads.has(id)),
    };
  }

  private emailQuery(raw: Args): Args {
    const args = parse(queryArgsSchema, raw);
    const filter = inMailboxSchema.safeParse(args.filter);
    if (!filter.success) {
      throw new MethodError("unsupportedFilter");
    }
    const box = filter.data.inMailbox;
    const ascending = direction(args.sort ?? []);
    const rows = [...this.server.emails.values()]
      .filter((row) => box in row.mailboxIds)
      .toSorted((a, b) => order(a, b, ascending));
    const ids = args.collapseThreads === true ? collapse(rows) : rows.map((row) => row.id);
    const position = start(args, ids);
    const asked = args.limit ?? this.server.queryLimit;
    const limit = Math.min(asked, this.server.queryLimit);
    const answer: Args = {
      accountId: UPSTREAM,
      queryState: this.state(),
      canCalculateChanges: false,
      position,
      ids: ids.slice(position, position + limit),
    };
    if (args.calculateTotal === true) {
      answer["total"] = ids.length;
    }
    if (limit < asked) {
      answer["limit"] = limit;
    }
    return answer;
  }

  private changes(type: Type, raw: Args): Args {
    const since = Number(parse(changesArgsSchema, raw).sinceState);
    if (!Number.isInteger(since) || since < this.server.horizon) {
      throw new MethodError("cannotCalculateChanges");
    }
    const rows = this.server.changes.filter((row) => row.type === type && row.sequence > since);
    const boundary = rows.at(this.server.changesCap - 1)?.sequence ?? Number.POSITIVE_INFINITY;
    const taken = rows.filter((row) => row.sequence <= boundary);
    const more = taken.length < rows.length;
    const newState = more ? (taken.at(-1)?.sequence ?? since) : this.server.sequence;
    const fates = [...fold(taken)];
    const answer: Args = {
      accountId: UPSTREAM,
      oldState: String(since),
      newState: String(newState),
      hasMoreChanges: more,
      created: fates.filter(([, kind]) => kind === "created").map(([id]) => id),
      updated: fates.filter(([, kind]) => kind === "updated").map(([id]) => id),
      destroyed: fates.filter(([, kind]) => kind === "destroyed").map(([id]) => id),
    };
    if (type === "Mailbox") {
      answer["updatedProperties"] = null;
    }
    return answer;
  }
}

// RFC 8620 section 5.5: an anchor wins over the position.
function start(args: z.infer<typeof queryArgsSchema>, ids: readonly string[]): number {
  if (args.anchor === undefined) {
    return args.position ?? 0;
  }
  const at = ids.indexOf(args.anchor);
  if (at < 0) {
    throw new MethodError("anchorNotFound");
  }
  return Math.max(0, at + (args.anchorOffset ?? 0));
}

type Fate = Kind | "gone";

// RFC 8620 section 5.2: created then destroyed is gone, created then
// anything is created, destroyed then created is updated and destroyed
// wins otherwise.
function then(before: Fate | undefined, kind: Kind): Fate {
  if (before === undefined) {
    return kind;
  }
  if (before === "created") {
    return kind === "destroyed" ? "gone" : "created";
  }
  if (before === "destroyed") {
    return kind === "created" ? "updated" : "destroyed";
  }
  return kind === "destroyed" ? "destroyed" : before;
}

function fold(rows: readonly Change[]): Map<string, Kind> {
  const fates = new Map<string, Fate>();
  for (const row of rows) {
    fates.set(row.id, then(fates.get(row.id), row.kind));
  }
  const listed: [string, Kind][] = [];
  for (const [id, fate] of fates) {
    if (fate !== "gone") {
      listed.push([id, fate]);
    }
  }
  return new Map(listed);
}

// Every thread's emails, oldest first, as the server keeps them.
function threadsOf(emails: ReadonlyMap<string, EmailHeader>): Map<string, string[]> {
  const threads = new Map<string, EmailHeader[]>();
  for (const row of emails.values()) {
    threads.set(row.threadId, [...(threads.get(row.threadId) ?? []), row]);
  }
  return new Map(
    [...threads].map(([id, rows]) => [
      id,
      rows.toSorted((a, b) => order(a, b, true)).map((row) => row.id),
    ]),
  );
}

function order(a: EmailHeader, b: EmailHeader, ascending: boolean): number {
  const byTime = Date.parse(a.receivedAt) - Date.parse(b.receivedAt);
  const sign = ascending ? 1 : -1;
  return byTime === 0 ? sign * a.id.localeCompare(b.id) : sign * byTime;
}

function direction(sort: readonly z.infer<typeof comparatorSchema>[]): boolean {
  const [one, ...rest] = sort;
  if (one === undefined) {
    return false;
  }
  if (rest.length > 0 || one.property !== "receivedAt") {
    throw new MethodError("unsupportedSort");
  }
  return one.isAscending ?? true;
}

// One email per thread, the first in the order given.
function collapse(rows: readonly EmailHeader[]): string[] {
  const seen = new Set<string>();
  return rows
    .filter((row) => {
      if (seen.has(row.threadId)) {
        return false;
      }
      seen.add(row.threadId);
      return true;
    })
    .map((row) => row.id);
}

function pick(row: EmailHeader, properties: readonly string[] | null): Args {
  if (properties === null) {
    return { ...row };
  }
  const entries = new Map<string, unknown>(Object.entries(row));
  return Object.fromEntries(["id", ...properties].map((name) => [name, entries.get(name)]));
}
