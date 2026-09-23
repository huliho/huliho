// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { ObjectType } from "../jmap/calls";
import type { EmailHeader, Mailbox } from "../jmap/schemas";
import type { Batch, MailStore, QueryRow, StoreArea, ThreadRow } from "./store";

const OBJECT_TYPES: readonly ObjectType[] = ["Mailbox", "Email", "Thread"];

function isObjectType(key: string): key is ObjectType {
  return (OBJECT_TYPES as readonly string[]).includes(key);
}

interface Rows {
  mailboxes: Map<string, Mailbox>;
  emails: Map<string, EmailHeader>;
  threads: Map<string, ThreadRow>;
  queries: Map<string, QueryRow>;
  states: Map<ObjectType, string>;
}

function emptyRows(): Rows {
  return {
    mailboxes: new Map(),
    emails: new Map(),
    threads: new Map(),
    queries: new Map(),
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

function clear(rows: Rows, area: StoreArea): void {
  if (area === "mailboxes") {
    rows.mailboxes.clear();
  } else if (area === "emails") {
    rows.emails.clear();
  } else if (area === "threads") {
    rows.threads.clear();
  } else {
    rows.queries.clear();
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

// The store that holds everything in memory: the tests run on it and an
// instance that keeps nothing on disk can run on it.
export class MemoryMailStore implements MailStore {
  private readonly accounts = new Map<string, Rows>();

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

  commit(accountId: string, batch: Batch): Promise<void> {
    const rows = this.rows(accountId);
    for (const area of batch.reset ?? []) {
      clear(rows, area);
    }
    apply(rows.mailboxes, batch.mailboxes);
    apply(rows.emails, batch.emails);
    apply(rows.threads, batch.threads);
    apply(rows.queries, batch.queries);
    for (const [type, state] of Object.entries(batch.states ?? {})) {
      if (!isObjectType(type)) {
        continue;
      }
      if (state === null) {
        rows.states.delete(type);
      } else {
        rows.states.set(type, state);
      }
    }
    return Promise.resolve();
  }

  private rows(accountId: string): Rows {
    const held = this.accounts.get(accountId);
    if (held !== undefined) {
      return held;
    }
    const fresh = emptyRows();
    this.accounts.set(accountId, fresh);
    return fresh;
  }
}
