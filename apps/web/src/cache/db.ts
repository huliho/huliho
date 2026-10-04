// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import {
  emailBodySchema,
  emailHeaderSchema,
  mailboxSchema,
  pendingRowSchema,
  queryRowSchema,
  threadRowSchema,
} from "@huliho/core";
import type {
  Batch,
  BodySize,
  EmailBody,
  EmailHeader,
  MailStore,
  Mailbox,
  ObjectType,
  PendingRow,
  QueryRow,
  StoreArea,
  ThreadRow,
} from "@huliho/core";
import { Dexie } from "dexie";
import type { DexieOptions, IndexableType, Table } from "dexie";

const DATABASE_NAME = "huliho-mail";

// A row under its account and object id; the pair is the primary key.
interface Keyed<Row> {
  accountId: string;
  id: string;
  row: Row;
}

// A body with what the eviction orders by beside the row, so its order
// is read from the index and no value leaves the disk for it.
interface StoredBody extends Keyed<EmailBody> {
  fetchedAt: number;
  bytes: number;
}

// A pending change under its account and sequence number.
interface StoredPending {
  accountId: string;
  seq: number;
  row: PendingRow;
}

type Stored<Row> = Table<Keyed<Row>, [string, string]>;

const KEYS = "[accountId+id], accountId";
const BODY_ORDER = "[fetchedAt+bytes]";

// The tables Dexie sets up from the schema; the fields only name them.
export class MailDatabase extends Dexie {
  declare readonly mailboxes: Stored<Mailbox>;
  declare readonly emailHeaders: Stored<EmailHeader>;
  declare readonly threads: Stored<ThreadRow>;
  declare readonly queryCache: Stored<QueryRow>;
  // One row per object type: its state string.
  declare readonly meta: Stored<string>;
  declare readonly emailBodies: Table<StoredBody, [string, string]>;
  declare readonly pendingChanges: Table<StoredPending, [string, number]>;

  // The options name an IndexedDB other than the global one, for a test.
  constructor(name = DATABASE_NAME, options?: DexieOptions) {
    super(name, options);
    this.version(1).stores({
      mailboxes: KEYS,
      emailHeaders: KEYS,
      threads: KEYS,
      queryCache: KEYS,
      meta: KEYS,
    });
    // The second version adds the bodies and the patch log.
    this.version(2).stores({
      emailBodies: `${KEYS}, ${BODY_ORDER}`,
      pendingChanges: "[accountId+seq], accountId",
    });
  }
}

// What a row read from disk is checked against before a caller sees it.
interface RowSchema<Row> {
  safeParse(value: unknown): { success: true; data: Row } | { success: false; error: unknown };
}

interface Change<Row extends { id: string }> {
  put?: readonly Row[];
  remove?: readonly string[];
}

// The row when it passes its schema. A row that fails is left out and
// named, so the caller fetches it anew instead of reading a wrong one.
function checked<Row>(table: string, row: unknown, schema: RowSchema<Row>): Row[] {
  const read = schema.safeParse(row);
  if (read.success) {
    return [read.data];
  }
  console.error(`cache: a row in ${table} failed its schema and was dropped`);
  return [];
}

// The rows that pass their schema, by id.
function parsed<Row>(
  table: string,
  rows: readonly (Keyed<unknown> | undefined)[],
  schema: RowSchema<Row>,
): Map<string, Row> {
  return new Map(
    rows.flatMap((held) =>
      held === undefined
        ? []
        : checked(table, held.row, schema).map((row): [string, Row] => [held.id, row]),
    ),
  );
}

function keysOf(accountId: string, ids: readonly string[]): [string, string][] {
  return ids.map((id): [string, string] => [accountId, id]);
}

async function apply<Row extends { id: string }>(
  table: Stored<Row>,
  accountId: string,
  change: Change<Row> | undefined,
): Promise<void> {
  await table.bulkDelete(keysOf(accountId, change?.remove ?? []));
  await table.bulkPut((change?.put ?? []).map((row) => ({ accountId, id: row.id, row })));
}

// A key of the eviction's index with the primary key it stands for.
function sizeOf(key: IndexableType, [accountId, id]: [string, string]): BodySize[] {
  if (!Array.isArray(key)) {
    return [];
  }
  const [fetchedAt, bytes] = key;
  return typeof fetchedAt === "number" && typeof bytes === "number"
    ? [{ accountId, id, fetchedAt, bytes }]
    : [];
}

// The store on IndexedDB through Dexie: one database per origin, every
// row under its account id, every batch one transaction.
export class DexieMailStore implements MailStore {
  private readonly db: MailDatabase;

  constructor(db: MailDatabase) {
    this.db = db;
  }

  async mailboxes(accountId: string): Promise<Mailbox[]> {
    const rows = await this.db.mailboxes.where("accountId").equals(accountId).toArray();
    return [...parsed("mailboxes", rows, mailboxSchema).values()];
  }

  async emails(accountId: string, ids: readonly string[]): Promise<Map<string, EmailHeader>> {
    const rows = await this.db.emailHeaders.bulkGet(keysOf(accountId, ids));
    return parsed("emailHeaders", rows, emailHeaderSchema);
  }

  async threads(accountId: string, ids: readonly string[]): Promise<Map<string, ThreadRow>> {
    const rows = await this.db.threads.bulkGet(keysOf(accountId, ids));
    return parsed("threads", rows, threadRowSchema);
  }

  async query(accountId: string, mailboxId: string): Promise<QueryRow | null> {
    const row = await this.db.queryCache.get([accountId, mailboxId]);
    return parsed("queryCache", [row], queryRowSchema).get(mailboxId) ?? null;
  }

  async queries(accountId: string): Promise<QueryRow[]> {
    const rows = await this.db.queryCache.where("accountId").equals(accountId).toArray();
    return [...parsed("queryCache", rows, queryRowSchema).values()];
  }

  async state(accountId: string, type: ObjectType): Promise<string | null> {
    const held = await this.db.meta.get([accountId, type]);
    return typeof held?.row === "string" ? held.row : null;
  }

  async body(accountId: string, emailId: string): Promise<EmailBody | null> {
    const row = await this.db.emailBodies.get([accountId, emailId]);
    return parsed("emailBodies", [row], emailBodySchema).get(emailId) ?? null;
  }

  async bodySizes(): Promise<BodySize[]> {
    const sizes: BodySize[] = [];
    await this.db.emailBodies.orderBy(BODY_ORDER).eachKey((key, cursor) => {
      sizes.push(...sizeOf(key, cursor.primaryKey));
    });
    return sizes;
  }

  async pending(accountId: string): Promise<PendingRow[]> {
    const rows = await this.db.pendingChanges.where("accountId").equals(accountId).sortBy("seq");
    return rows.flatMap((held) => checked("pendingChanges", held.row, pendingRowSchema));
  }

  commit(accountId: string, batch: Batch): Promise<void> {
    const { db } = this;
    return db.transaction("rw", db.tables, async () => {
      await Promise.all(
        (batch.reset ?? []).map((area) =>
          this.table(area).where("accountId").equals(accountId).delete(),
        ),
      );
      await apply(db.mailboxes, accountId, batch.mailboxes);
      await apply(db.emailHeaders, accountId, batch.emails);
      await apply(db.threads, accountId, batch.threads);
      await apply(db.queryCache, accountId, batch.queries);
      await this.applyBodies(accountId, batch.bodies);
      await this.applyPending(accountId, batch.pending);
      await Promise.all(
        Object.entries(batch.states ?? {}).map(([type, state]) =>
          state === null
            ? db.meta.delete([accountId, type])
            : db.meta.put({ accountId, id: type, row: state }),
        ),
      );
    });
  }

  // The account ids any table holds a row under.
  async accounts(): Promise<string[]> {
    const keys = await Promise.all(
      this.db.tables.map((table) => table.orderBy("accountId").uniqueKeys()),
    );
    return [...new Set(keys.flat().filter((key) => typeof key === "string"))];
  }

  // Deletes the database; the next call opens an empty one.
  destroy(): Promise<void> {
    return this.db.delete({ disableAutoOpen: false });
  }

  private async applyBodies(accountId: string, change: Batch["bodies"]): Promise<void> {
    const { emailBodies } = this.db;
    await emailBodies.bulkDelete(keysOf(accountId, change?.remove ?? []));
    await emailBodies.bulkPut(
      (change?.put ?? []).map((row) => ({
        accountId,
        id: row.id,
        fetchedAt: row.fetchedAt,
        bytes: row.bytes,
        row,
      })),
    );
  }

  private async applyPending(accountId: string, change: Batch["pending"]): Promise<void> {
    const { pendingChanges } = this.db;
    await pendingChanges.bulkDelete(
      (change?.remove ?? []).map((seq): [string, number] => [accountId, seq]),
    );
    await pendingChanges.bulkPut(
      (change?.put ?? []).map((row) => ({ accountId, seq: row.seq, row })),
    );
  }

  private table(area: StoreArea): Table {
    switch (area) {
      case "mailboxes":
        return this.db.mailboxes;
      case "emails":
        return this.db.emailHeaders;
      case "threads":
        return this.db.threads;
      case "queries":
        return this.db.queryCache;
      case "bodies":
        return this.db.emailBodies;
      default:
        return this.db.pendingChanges;
    }
  }
}
