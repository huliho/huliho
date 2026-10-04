// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ObjectType } from "../jmap/calls";
import type { EmailHeader, Mailbox } from "../jmap/schemas";
import type {
  Batch,
  BodySize,
  EmailBody,
  MailStore,
  PendingRow,
  QueryRow,
  StoreArea,
  ThreadRow,
} from "./store";

const OBJECT_TYPES: readonly ObjectType[] = ["Mailbox", "Email", "Thread"];

function isObjectType(key: string): key is ObjectType {
  return (OBJECT_TYPES as readonly string[]).includes(key);
}

interface Rows {
  mailboxes: Map<string, Mailbox>;
  emails: Map<string, EmailHeader>;
  threads: Map<string, ThreadRow>;
  queries: Map<string, QueryRow>;
  bodies: Map<string, EmailBody>;
  pending: Map<number, PendingRow>;
  states: Map<ObjectType, string>;
}

function emptyRows(): Rows {
  return {
    mailboxes: new Map(),
    emails: new Map(),
    threads: new Map(),
    queries: new Map(),
    bodies: new Map(),
    pending: new Map(),
    states: new Map(),
  };
}

function pick<Row>(rows: Map<string, Row>, ids: readonly string[]): Map<string, Row> {
  const found = new Map<string, Row>();
  for (const id of ids) {
    const row = rows.get(id);
    if (row !== undefined) {
      found.set(id, structuredClone(row));
    }
  }
  return found;
}

function areaOf(rows: Rows, area: StoreArea): Map<string, unknown> | Map<number, unknown> {
  switch (area) {
    case "mailboxes":
      return rows.mailboxes;
    case "emails":
      return rows.emails;
    case "threads":
      return rows.threads;
    case "queries":
      return rows.queries;
    case "bodies":
      return rows.bodies;
    default:
      return rows.pending;
  }
}

function apply<Row extends { id: string }>(
  rows: Map<string, Row>,
  change: { put?: readonly Row[]; remove?: readonly string[] } | undefined,
): void {
  for (const id of change?.remove ?? []) {
    rows.delete(id);
  }
  for (const row of change?.put ?? []) {
    rows.set(row.id, structuredClone(row));
  }
}

function applyPending(rows: Map<number, PendingRow>, change: Batch["pending"]): void {
  for (const seq of change?.remove ?? []) {
    rows.delete(seq);
  }
  for (const row of change?.put ?? []) {
    rows.set(row.seq, structuredClone(row));
  }
}

function applyStates(states: Map<ObjectType, string>, change: Batch["states"]): void {
  for (const [type, state] of Object.entries(change ?? {})) {
    if (!isObjectType(type)) {
      continue;
    }
    if (state === null) {
      states.delete(type);
    } else {
      states.set(type, state);
    }
  }
}

function isEmpty(rows: Rows): boolean {
  return Object.values(rows).every((area: Map<unknown, unknown>) => area.size === 0);
}

// The store that holds everything in memory: the tests run on it and an
// instance that keeps nothing on disk can run on it.
export class MemoryMailStore implements MailStore {
  private readonly held = new Map<string, Rows>();

  mailboxes(accountId: string): Promise<Mailbox[]> {
    return Promise.resolve(
      [...this.rows(accountId).mailboxes.values()].map((row) => structuredClone(row)),
    );
  }

  emails(accountId: string, ids: readonly string[]): Promise<Map<string, EmailHeader>> {
    return Promise.resolve(pick(this.rows(accountId).emails, ids));
  }

  threads(accountId: string, ids: readonly string[]): Promise<Map<string, ThreadRow>> {
    return Promise.resolve(pick(this.rows(accountId).threads, ids));
  }

  query(accountId: string, mailboxId: string): Promise<QueryRow | null> {
    const row = this.rows(accountId).queries.get(mailboxId);
    return Promise.resolve(row === undefined ? null : structuredClone(row));
  }

  queries(accountId: string): Promise<QueryRow[]> {
    return Promise.resolve(
      [...this.rows(accountId).queries.values()].map((row) => structuredClone(row)),
    );
  }

  state(accountId: string, type: ObjectType): Promise<string | null> {
    return Promise.resolve(this.rows(accountId).states.get(type) ?? null);
  }

  body(accountId: string, emailId: string): Promise<EmailBody | null> {
    const row = this.rows(accountId).bodies.get(emailId);
    return Promise.resolve(row === undefined ? null : structuredClone(row));
  }

  bodySizes(): Promise<BodySize[]> {
    const sizes = [...this.held].flatMap(([accountId, rows]) =>
      [...rows.bodies.values()].map(({ id, fetchedAt, bytes }) => ({
        accountId,
        id,
        fetchedAt,
        bytes,
      })),
    );
    return Promise.resolve(sizes);
  }

  pending(accountId: string): Promise<PendingRow[]> {
    const rows = [...this.rows(accountId).pending.values()].map((row) => structuredClone(row));
    return Promise.resolve(rows.toSorted((a, b) => a.seq - b.seq));
  }

  commit(accountId: string, batch: Batch): Promise<void> {
    const rows = this.rows(accountId);
    for (const area of batch.reset ?? []) {
      areaOf(rows, area).clear();
    }
    apply(rows.mailboxes, batch.mailboxes);
    apply(rows.emails, batch.emails);
    apply(rows.threads, batch.threads);
    apply(rows.queries, batch.queries);
    apply(rows.bodies, batch.bodies);
    applyPending(rows.pending, batch.pending);
    applyStates(rows.states, batch.states);
    return Promise.resolve();
  }

  // The account ids that hold a row.
  accounts(): Promise<string[]> {
    const ids = [...this.held].filter(([, rows]) => !isEmpty(rows)).map(([id]) => id);
    return Promise.resolve(ids);
  }

  // Forgets every row of every account.
  destroy(): Promise<void> {
    this.held.clear();
    return Promise.resolve();
  }

  private rows(accountId: string): Rows {
    const held = this.held.get(accountId);
    if (held !== undefined) {
      return held;
    }
    const fresh = emptyRows();
    this.held.set(accountId, fresh);
    return fresh;
  }
}
