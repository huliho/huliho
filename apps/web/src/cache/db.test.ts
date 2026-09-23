// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// @vitest-environment node

import { ACCOUNT, at, email, mailbox } from "@huliho/core/testing";
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
