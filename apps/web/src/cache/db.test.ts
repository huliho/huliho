// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// @vitest-environment node

import type { EmailBody, PendingRow } from "@huliho/core";
import { ACCOUNT, at, email, mailbox } from "@huliho/core/testing";
import { Dexie } from "dexie";
import { IDBFactory, IDBKeyRange } from "fake-indexeddb";
import { afterEach, expect, test, vi } from "vitest";

import { DexieMailStore, MailDatabase } from "./db";

const OTHER = "acc-2";

// A database of its own per test, on an IndexedDB held in memory.
function fresh(): MailDatabase {
  return new MailDatabase("huliho-test", { indexedDB: new IDBFactory(), IDBKeyRange });
}

let database = fresh();
let store = new DexieMailStore(database);

afterEach(() => {
  database.close();
  database = fresh();
  store = new DexieMailStore(database);
  vi.restoreAllMocks();
});

test("a batch lands under its account and a read answers a copy", async () => {
  const inbox = mailbox("inbox", "inbox");
  const held = email("e1", { receivedAt: at(1) });
  await store.commit(ACCOUNT, {
    mailboxes: { put: [inbox] },
    emails: { put: [held] },
    threads: { put: [{ id: "t-e1", emailIds: ["e1"], members: {} }] },
    queries: {
      put: [{ id: "inbox", queryState: "3", total: 1, pages: [], pending: [], fresh: null }],
    },
    states: { Mailbox: "3", Email: "3" },
  });
  const read = await store.mailboxes(ACCOUNT);
  expect(read).toEqual([inbox]);
  for (const row of read) {
    row.name = "changed";
  }
  expect((await store.mailboxes(ACCOUNT))[0]?.name).toBe("inbox");
  expect((await store.emails(ACCOUNT, ["e1", "e9"])).get("e1")?.subject).toBe("Message e1");
  expect((await store.threads(ACCOUNT, ["t-e1"])).get("t-e1")?.emailIds).toEqual(["e1"]);
  expect((await store.query(ACCOUNT, "inbox"))?.total).toBe(1);
  expect(await store.queries(ACCOUNT)).toHaveLength(1);
  expect(await store.state(ACCOUNT, "Mailbox")).toBe("3");
  expect(await store.state(ACCOUNT, "Thread")).toBeNull();
  expect(await store.mailboxes(OTHER)).toEqual([]);
  expect(await store.query(OTHER, "inbox")).toBeNull();
  expect(await store.state(OTHER, "Mailbox")).toBeNull();
});

test("a batch resets, removes, puts and moves the states in that order", async () => {
  await store.commit(ACCOUNT, {
    emails: { put: [email("e1", { receivedAt: at(1) }), email("e2", { receivedAt: at(2) })] },
    states: { Email: "2", Thread: "2" },
  });
  await store.commit(OTHER, { emails: { put: [email("e1", { receivedAt: at(1) })] } });
  await store.commit(ACCOUNT, {
    reset: ["emails"],
    emails: { put: [email("e3", { receivedAt: at(3) })], remove: ["e3"] },
    states: { Email: "5", Thread: null },
  });
  expect([...(await store.emails(ACCOUNT, ["e1", "e2", "e3"])).keys()]).toEqual(["e3"]);
  expect((await store.emails(OTHER, ["e1"])).size).toBe(1);
  expect((await store.accounts()).toSorted()).toEqual([ACCOUNT, OTHER].toSorted());
  expect(await store.state(ACCOUNT, "Email")).toBe("5");
  expect(await store.state(ACCOUNT, "Thread")).toBeNull();
});

test("a batch that fails lands nothing", async () => {
  await store.commit(ACCOUNT, { states: { Email: "1" } });
  // A function cannot be cloned into IndexedDB, so the put fails.
  const broken = Object.assign(email("e1", { receivedAt: at(1) }), { preview: () => "" });
  await expect(
    store.commit(ACCOUNT, { emails: { put: [broken] }, states: { Email: "2" } }),
  ).rejects.toThrow(/clone/i);
  expect(await store.state(ACCOUNT, "Email")).toBe("1");
});

test("a row that fails its schema is dropped and named", async () => {
  const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
  await store.commit(ACCOUNT, { emails: { put: [email("e1", { receivedAt: at(1) })] } });
  await database.table("emailHeaders").put({ accountId: ACCOUNT, id: "e2", row: { id: "e2" } });
  const read = await store.emails(ACCOUNT, ["e1", "e2"]);
  expect([...read.keys()]).toEqual(["e1"]);
  expect(error).toHaveBeenCalledWith(
    "cache: a row in emailHeaders failed its schema and was dropped",
  );
});

test("destroy empties the database for the next call", async () => {
  await store.commit(ACCOUNT, { states: { Email: "1" } });
  await store.destroy();
  expect(await store.state(ACCOUNT, "Email")).toBeNull();
});

const PART = {
  partId: "1",
  blobId: "b1",
  size: 5,
  name: null,
  type: "text/plain",
  charset: "utf-8",
  disposition: null,
  cid: null,
  language: null,
  location: null,
};

// A stored body of a chosen age and weight.
function body(id: string, fetchedAt: number, bytes: number): EmailBody {
  return {
    id,
    bodyStructure: PART,
    textBody: [PART],
    htmlBody: [PART],
    attachments: [],
    bodyValues: { "1": { value: "Hello", isEncodingProblem: false, isTruncated: false } },
    authentication: { status: "absent" },
    flowed: null,
    large: false,
    fetchedAt,
    bytes,
  };
}

const CHANGE: PendingRow = {
  seq: 1,
  type: "Email",
  id: "e1",
  patch: { "keywords/$seen": true },
  inverse: { "keywords/$seen": null },
  sentAt: null,
};

test("a body and the pending changes land under their account and leave by a batch", async () => {
  await store.commit(ACCOUNT, {
    bodies: { put: [body("e1", 10, 5)] },
    pending: { put: [{ ...CHANGE, seq: 2 }, CHANGE] },
  });
  await store.commit(OTHER, { pending: { put: [CHANGE] } });
  expect(await store.body(ACCOUNT, "e1")).toEqual(body("e1", 10, 5));
  expect(await store.body(OTHER, "e1")).toBeNull();
  expect((await store.pending(ACCOUNT)).map((row) => row.seq)).toEqual([1, 2]);
  expect((await store.accounts()).toSorted()).toEqual([ACCOUNT, OTHER].toSorted());
  await store.commit(ACCOUNT, { bodies: { remove: ["e1"] }, pending: { remove: [1] } });
  expect(await store.body(ACCOUNT, "e1")).toBeNull();
  expect(await store.pending(ACCOUNT)).toEqual([{ ...CHANGE, seq: 2 }]);
  expect(await store.pending(OTHER)).toEqual([CHANGE]);
  await store.commit(ACCOUNT, {
    reset: ["bodies", "pending"],
    bodies: { put: [body("e2", 1, 1)] },
  });
  expect(await store.pending(ACCOUNT)).toEqual([]);
  expect((await store.bodySizes()).map((size) => size.id)).toEqual(["e2"]);
});

test("the order the eviction reads comes from the index, the oldest first across accounts", async () => {
  await store.commit(ACCOUNT, { bodies: { put: [body("late", 20, 2), body("early", 10, 1)] } });
  await store.commit(OTHER, { bodies: { put: [body("between", 15, 3)] } });
  expect(await store.bodySizes()).toEqual([
    { accountId: ACCOUNT, id: "early", fetchedAt: 10, bytes: 1 },
    { accountId: OTHER, id: "between", fetchedAt: 15, bytes: 3 },
    { accountId: ACCOUNT, id: "late", fetchedAt: 20, bytes: 2 },
  ]);
});

test("a stored body or pending change that fails its schema is dropped and named", async () => {
  const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
  await store.commit(ACCOUNT, { pending: { put: [CHANGE] } });
  await database.table("emailBodies").put({ accountId: ACCOUNT, id: "e1", row: { id: "e1" } });
  await database.table("pendingChanges").put({ accountId: ACCOUNT, seq: 2, row: { seq: 2 } });
  // A path that names no keyword would fail every later read of the log.
  const moved = { ...CHANGE, seq: 3, patch: { "mailboxIds/archive": true } };
  await database.table("pendingChanges").put({ accountId: ACCOUNT, seq: 3, row: moved });
  expect(await store.body(ACCOUNT, "e1")).toBeNull();
  expect(await store.pending(ACCOUNT)).toEqual([CHANGE]);
  expect(error).toHaveBeenCalledWith(
    "cache: a row in emailBodies failed its schema and was dropped",
  );
  expect(error).toHaveBeenCalledWith(
    "cache: a row in pendingChanges failed its schema and was dropped",
  );
});

test("a database of the first version keeps its rows and gains the two tables", async () => {
  const options = { indexedDB: new IDBFactory(), IDBKeyRange };
  const first = new Dexie("huliho-upgrade", options);
  first.version(1).stores({
    mailboxes: "[accountId+id], accountId",
    emailHeaders: "[accountId+id], accountId",
    threads: "[accountId+id], accountId",
    queryCache: "[accountId+id], accountId",
    meta: "[accountId+id], accountId",
  });
  await first.table("meta").put({ accountId: ACCOUNT, id: "Email", row: "7" });
  first.close();
  const upgraded = new MailDatabase("huliho-upgrade", options);
  const reopened = new DexieMailStore(upgraded);
  expect(await reopened.state(ACCOUNT, "Email")).toBe("7");
  await reopened.commit(ACCOUNT, { bodies: { put: [body("e1", 1, 5)] } });
  expect(await reopened.body(ACCOUNT, "e1")).toEqual(body("e1", 1, 5));
  upgraded.close();
});
