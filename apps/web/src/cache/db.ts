// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import {
  emailBodySchema,
  emailHeaderSchema,
  heldQuotaSchema,
  mailboxSchema,
  pendingRowSchema,
  queryRowSchema,
  sendersOf,
  threadRowSchema,
} from "@huliho/core";
import type {
  Batch,
  BodySize,
  EmailBody,
  EmailHeader,
  HeldQuota,
  MailStore,
  Mailbox,
  ObjectType,
  PendingRow,
  QueryRow,
  SearchRow,
  Sender,
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

// The meta row that holds the account's quotas beside the state strings.
const QUOTA_KEY = "quota";

// The tables Dexie sets up from the schema; the fields only name them.
export class MailDatabase extends Dexie {
  declare readonly mailboxes: Stored<Mailbox>;
  declare readonly emailHeaders: Stored<EmailHeader>;
  declare readonly threads: Stored<ThreadRow>;
  declare readonly queryCache: Stored<QueryRow>;
  // One row per object type with its state string, plus the quotas.
  declare readonly meta: Stored<unknown>;
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

// The areas that live on disk; a search is held in memory alone.
type DiskArea = Exclude<StoreArea, "searches">;

function onDisk(area: StoreArea): area is DiskArea {
  return area !== "searches";
}

// What the senders fold from: the two fields of a header they read.
type SenderFact = Pick<EmailHeader, "from" | "receivedAt">;

function factOf(row: EmailHeader): SenderFact {
  return {
    from: row.from === null ? null : row.from.map((address) => ({ ...address })),
    receivedAt: row.receivedAt,
  };
}

// The store on IndexedDB through Dexie: one database per origin, every
// row under its account id, every batch one transaction. The searches
// stay in memory beside it, so a search never reaches the disk.
export class DexieMailStore implements MailStore {
  private readonly db: MailDatabase;
  private readonly searchRows = new Map<string, Map<string, SearchRow>>();
  // The sender facts of each account by email id, read from the table
  // once and kept up by every batch since.
  private readonly senderFacts = new Map<string, Map<string, SenderFact>>();
  // How many batches changed the account's headers, so a read a batch
  // overtook is not kept.
  private readonly headerBatches = new Map<string, number>();

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

  search(accountId: string, key: string): Promise<SearchRow | null> {
    const row = this.searchRows.get(accountId)?.get(key);
    return Promise.resolve(row === undefined ? null : structuredClone(row));
  }

  searches(accountId: string): Promise<SearchRow[]> {
    const rows = [...(this.searchRows.get(accountId)?.values() ?? [])];
    return Promise.resolve(rows.map((row) => structuredClone(row)));
  }

  async senders(accountId: string): Promise<Sender[]> {
    const held = this.senderFacts.get(accountId) ?? (await this.readSenderFacts(accountId));
    return sendersOf(held.values());
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

  async quota(accountId: string): Promise<HeldQuota | null> {
    const held = await this.db.meta.get([accountId, QUOTA_KEY]);
    return held === undefined ? null : (checked("meta", held.row, heldQuotaSchema)[0] ?? null);
  }

  async commit(accountId: string, batch: Batch): Promise<void> {
    const { db } = this;
    const reset = batch.reset ?? [];
    await db.transaction("rw", db.tables, async () => {
      await Promise.all(
        reset
          .filter(onDisk)
          .map((area) => this.table(area).where("accountId").equals(accountId).delete()),
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
      await this.applyQuota(accountId, batch.quota);
    });
    this.applySearches(accountId, batch);
    if (batch.emails !== undefined || reset.includes("emails")) {
      this.headerBatches.set(accountId, (this.headerBatches.get(accountId) ?? 0) + 1);
      this.applySenderFacts(accountId, batch);
    }
  }

  // The account ids any table holds a row under, or a search.
  async accounts(): Promise<string[]> {
    const keys = await Promise.all(
      this.db.tables.map((table) => table.orderBy("accountId").uniqueKeys()),
    );
    const searching = [...this.searchRows].filter(([, rows]) => rows.size > 0).map(([id]) => id);
    return [...new Set([...keys.flat().filter((key) => typeof key === "string"), ...searching])];
  }

  // Deletes the database and forgets the searches; the next call opens
  // an empty one.
  destroy(): Promise<void> {
    this.searchRows.clear();
    this.senderFacts.clear();
    return this.db.delete({ disableAutoOpen: false });
  }

  // The facts of every header in the table. A batch that lands during
  // the read leaves them unkept, so the next call reads the table again.
  private async readSenderFacts(accountId: string): Promise<Map<string, SenderFact>> {
    const batches = this.headerBatches.get(accountId) ?? 0;
    const rows = await this.db.emailHeaders.where("accountId").equals(accountId).toArray();
    const facts = new Map(
      [...parsed("emailHeaders", rows, emailHeaderSchema)].map(
        ([id, row]): [string, SenderFact] => [id, factOf(row)],
      ),
    );
    if ((this.headerBatches.get(accountId) ?? 0) === batches) {
      this.senderFacts.set(accountId, facts);
    }
    return facts;
  }

  // Folds a batch into the held facts: removed ids leave and put rows
  // take their place. A reset drops them, so the next read comes from
  // the table.
  private applySenderFacts(accountId: string, batch: Batch): void {
    const held = this.senderFacts.get(accountId);
    if (held === undefined) {
      return;
    }
    if (batch.reset?.includes("emails") === true) {
      this.senderFacts.delete(accountId);
      return;
    }
    for (const id of batch.emails?.remove ?? []) {
      held.delete(id);
    }
    for (const row of batch.emails?.put ?? []) {
      held.set(row.id, factOf(row));
    }
  }

  private applySearches(accountId: string, batch: Batch): void {
    const reset = batch.reset?.includes("searches") === true;
    const rows = new Map(reset ? [] : this.searchRows.get(accountId));
    for (const key of batch.searches?.remove ?? []) {
      rows.delete(key);
    }
    for (const row of batch.searches?.put ?? []) {
      rows.set(row.id, structuredClone(row));
    }
    this.searchRows.set(accountId, rows);
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

  private async applyQuota(accountId: string, quota: Batch["quota"]): Promise<void> {
    if (quota === null) {
      await this.db.meta.delete([accountId, QUOTA_KEY]);
    } else if (quota !== undefined) {
      await this.db.meta.put({ accountId, id: QUOTA_KEY, row: quota });
    }
  }

  private table(area: DiskArea): Table {
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
