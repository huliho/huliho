// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { emailHeaderSchema, mailboxSchema, queryRowSchema, threadRowSchema } from "@huliho/core";
import type {
  Batch,
  EmailHeader,
  MailStore,
  Mailbox,
  ObjectType,
  QueryRow,
  StoreArea,
  ThreadRow,
} from "@huliho/core";
import { Dexie } from "dexie";
import type { DexieOptions, Table } from "dexie";

const DATABASE_NAME = "huliho-mail";

// A row under its account and object id; the pair is the primary key.
interface Keyed<Row> {
  accountId: string;
  id: string;
  row: Row;
}

type Stored<Row> = Table<Keyed<Row>, [string, string]>;

type ObjectTable = Stored<Mailbox> | Stored<EmailHeader> | Stored<ThreadRow> | Stored<QueryRow>;

const KEYS = "[accountId+id], accountId";

// The tables Dexie sets up from the schema; the fields only name them.
export class MailDatabase extends Dexie {
  declare readonly mailboxes: Stored<Mailbox>;
  declare readonly emailHeaders: Stored<EmailHeader>;
  declare readonly threads: Stored<ThreadRow>;
  declare readonly queryCache: Stored<QueryRow>;
  // One row per object type: its state string.
  declare readonly meta: Stored<string>;

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

// The rows that pass their schema, by id. A row that fails is left out
// and named, so the caller fetches it anew instead of reading a wrong one.
function parsed<Row>(
  table: string,
  rows: readonly (Keyed<unknown> | undefined)[],
  schema: RowSchema<Row>,
): Map<string, Row> {
  const found = new Map<string, Row>();
  for (const held of rows) {
    if (held === undefined) {
      continue;
    }
    const read = schema.safeParse(held.row);
    if (read.success) {
      found.set(held.id, read.data);
    } else {
      console.error(`cache: a row in ${table} failed its schema and was dropped`);
    }
  }
  return found;
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

  commit(accountId: string, batch: Batch): Promise<void> {
    const { db } = this;
    const tables = [db.mailboxes, db.emailHeaders, db.threads, db.queryCache, db.meta];
    return db.transaction("rw", tables, async () => {
      await Promise.all(
        (batch.reset ?? []).map((area) =>
          this.table(area).where("accountId").equals(accountId).delete(),
        ),
      );
      await apply(db.mailboxes, accountId, batch.mailboxes);
      await apply(db.emailHeaders, accountId, batch.emails);
      await apply(db.threads, accountId, batch.threads);
      await apply(db.queryCache, accountId, batch.queries);
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
    const { db } = this;
    const tables = [db.mailboxes, db.emailHeaders, db.threads, db.queryCache, db.meta];
    const keys = await Promise.all(tables.map((table) => table.orderBy("accountId").uniqueKeys()));
    return [...new Set(keys.flat().filter((key) => typeof key === "string"))];
  }

  // Deletes the database; the next call opens an empty one.
  destroy(): Promise<void> {
    return this.db.delete({ disableAutoOpen: false });
  }

  private table(area: StoreArea): ObjectTable {
    if (area === "mailboxes") {
      return this.db.mailboxes;
    }
    if (area === "emails") {
      return this.db.emailHeaders;
    }
    return area === "threads" ? this.db.threads : this.db.queryCache;
  }
}
