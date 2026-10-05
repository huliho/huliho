// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { MemoryMailStore } from "@huliho/core";
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
  ThreadRow,
} from "@huliho/core";

// What the worker asks of its store beside the contract: the accounts
// that hold a row and a way to forget every row.
export interface WorkerStore extends MailStore {
  accounts(): Promise<string[]>;
  destroy(): Promise<void>;
}

// What naming the setting did to the store: nothing, opened the first
// one or replaced one in use with an empty one.
export type Choice = "kept" | "opened" | "replaced";

export interface Disk {
  // The store on disk.
  open(): WorkerStore;
  // Deletes the database on disk, whether or not one exists.
  erase(): Promise<void>;
}

// The store by the instance's privacy setting: the database on disk, or
// rows in memory with that database deleted. A call waits for the first
// choice. Once strict, the store stays in memory for the worker's life,
// so a tab with an older session answer cannot bring the disk back.
export class ChosenStore implements WorkerStore {
  private readonly disk: Disk;
  private readonly first = Promise.withResolvers<WorkerStore>();
  private store: Promise<WorkerStore> = this.first.promise;
  private strict: boolean | null = null;

  constructor(disk: Disk) {
    this.disk = disk;
  }

  // Takes the setting as a tab read it from the session answer. A store
  // that replaces one in use starts empty.
  choose(strict: boolean): Choice {
    if (this.strict === strict || this.strict === true) {
      return "kept";
    }
    const choice = this.strict === null ? "opened" : "replaced";
    this.strict = strict;
    this.store = strict ? this.inMemory() : Promise.resolve(this.disk.open());
    this.first.resolve(this.store);
    return choice;
  }

  mailboxes(accountId: string): Promise<Mailbox[]> {
    return this.store.then((store) => store.mailboxes(accountId));
  }

  emails(accountId: string, ids: readonly string[]): Promise<Map<string, EmailHeader>> {
    return this.store.then((store) => store.emails(accountId, ids));
  }

  threads(accountId: string, ids: readonly string[]): Promise<Map<string, ThreadRow>> {
    return this.store.then((store) => store.threads(accountId, ids));
  }

  query(accountId: string, mailboxId: string): Promise<QueryRow | null> {
    return this.store.then((store) => store.query(accountId, mailboxId));
  }

  queries(accountId: string): Promise<QueryRow[]> {
    return this.store.then((store) => store.queries(accountId));
  }

  state(accountId: string, type: ObjectType): Promise<string | null> {
    return this.store.then((store) => store.state(accountId, type));
  }

  body(accountId: string, emailId: string): Promise<EmailBody | null> {
    return this.store.then((store) => store.body(accountId, emailId));
  }

  bodySizes(): Promise<BodySize[]> {
    return this.store.then((store) => store.bodySizes());
  }

  pending(accountId: string): Promise<PendingRow[]> {
    return this.store.then((store) => store.pending(accountId));
  }

  commit(accountId: string, batch: Batch): Promise<void> {
    return this.store.then((store) => store.commit(accountId, batch));
  }

  accounts(): Promise<string[]> {
    return this.store.then((store) => store.accounts());
  }

  // A sign-out deletes the database whatever the setting, also before
  // any tab named one.
  destroy(): Promise<void> {
    if (this.strict === null) {
      return this.disk.erase();
    }
    return this.store.then((store) => store.destroy());
  }

  // The rows in memory, once the database on disk is gone. A delete that
  // fails is named and holds nothing up: no row is written to disk.
  private async inMemory(): Promise<WorkerStore> {
    try {
      await this.disk.erase();
    } catch (error) {
      console.error(
        "cache: deleting the stored mail failed",
        error instanceof Error ? error.message : String(error),
      );
    }
    return new MemoryMailStore();
  }
}
