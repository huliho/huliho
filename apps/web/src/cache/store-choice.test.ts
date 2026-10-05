// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// @vitest-environment node

import { mailbox } from "@huliho/core/testing";
import { IDBFactory, IDBKeyRange } from "fake-indexeddb";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { DexieMailStore, MailDatabase } from "./db";
import { ChosenStore } from "./store-choice";
import type { Disk } from "./store-choice";

const NAME = "huliho-test";
const INBOX = mailbox("inbox", "inbox");

let indexedDB = new IDBFactory();

function database(): MailDatabase {
  return new MailDatabase(NAME, { indexedDB, IDBKeyRange });
}

// The disk as the worker hands it over, with what was asked of it counted.
function disk() {
  return {
    open: vi.fn<Disk["open"]>(() => new DexieMailStore(database())),
    erase: vi.fn<Disk["erase"]>(() => database().delete()),
  };
}

async function stored(): Promise<string[]> {
  return (await indexedDB.databases()).map((row) => row.name ?? "");
}

// A database an earlier start left behind, with one row.
async function leftBehind(): Promise<void> {
  const earlier = new DexieMailStore(database());
  await earlier.commit("a1", { mailboxes: { put: [INBOX] } });
}

beforeEach(() => {
  indexedDB = new IDBFactory();
});

afterEach(() => {
  vi.restoreAllMocks();
});

test("no call reaches a store before a tab named the setting", async () => {
  const held = disk();
  const store = new ChosenStore(held);
  let answered = false;
  const read = store.mailboxes("a1").then((rows) => {
    answered = true;
    return rows;
  });
  await new Promise((resolve) => {
    setImmediate(resolve);
  });
  expect(answered).toBe(false);
  expect(held.open).not.toHaveBeenCalled();
  expect(await stored()).toEqual([]);
  expect(store.choose(false)).toBe("opened");
  expect(await read).toEqual([]);
  expect(held.open).toHaveBeenCalledOnce();
  expect(held.erase).not.toHaveBeenCalled();
});

test("an instance that stores mail keeps its rows on disk", async () => {
  const store = new ChosenStore(disk());
  store.choose(false);
  await store.commit("a1", { mailboxes: { put: [INBOX] } });
  expect(await stored()).toEqual([NAME]);
  expect(await new DexieMailStore(database()).mailboxes("a1")).toEqual([INBOX]);
  expect(await store.accounts()).toEqual(["a1"]);
});

test("a strict instance deletes the database an earlier start left and keeps its rows in memory", async () => {
  await leftBehind();
  expect(await stored()).toEqual([NAME]);
  const held = disk();
  const store = new ChosenStore(held);
  expect(store.choose(true)).toBe("opened");
  expect(await store.mailboxes("a1")).toEqual([]);
  expect(await stored()).toEqual([]);
  await store.commit("a1", { mailboxes: { put: [INBOX] } });
  expect(await store.mailboxes("a1")).toEqual([INBOX]);
  expect(await store.accounts()).toEqual(["a1"]);
  expect(await stored()).toEqual([]);
  expect(held.open).not.toHaveBeenCalled();
});

test("the same setting again changes nothing; a strict one replaces the store on disk with an empty one", async () => {
  const held = disk();
  const store = new ChosenStore(held);
  store.choose(false);
  await store.commit("a1", { mailboxes: { put: [INBOX] } });
  expect(store.choose(false)).toBe("kept");
  expect(await store.mailboxes("a1")).toEqual([INBOX]);
  expect(store.choose(true)).toBe("replaced");
  expect(await store.mailboxes("a1")).toEqual([]);
  expect(await stored()).toEqual([]);
  expect(held.open).toHaveBeenCalledOnce();
  expect(held.erase).toHaveBeenCalledOnce();
});

test("a strict store stays, whatever a tab with an older session answer names", async () => {
  const held = disk();
  const store = new ChosenStore(held);
  store.choose(true);
  await store.commit("a1", { mailboxes: { put: [INBOX] } });
  expect(store.choose(false)).toBe("kept");
  expect(store.choose(true)).toBe("kept");
  expect(await store.mailboxes("a1")).toEqual([INBOX]);
  expect(await stored()).toEqual([]);
  expect(held.open).not.toHaveBeenCalled();
  expect(held.erase).toHaveBeenCalledOnce();
});

test("a delete that fails is named and the rows still stay off the disk", async () => {
  const logged = vi.spyOn(console, "error").mockImplementation(() => undefined);
  const held = { ...disk(), erase: vi.fn<Disk["erase"]>(() => Promise.reject(new Error("no"))) };
  const store = new ChosenStore(held);
  store.choose(true);
  await store.commit("a1", { mailboxes: { put: [INBOX] } });
  expect(await store.mailboxes("a1")).toEqual([INBOX]);
  expect(await stored()).toEqual([]);
  expect(logged).toHaveBeenCalledExactlyOnceWith("cache: deleting the stored mail failed", "no");
});

test("a sign-out deletes the database before any setting and forgets the chosen store's rows after one", async () => {
  await leftBehind();
  const unchosen = new ChosenStore(disk());
  await unchosen.destroy();
  expect(await stored()).toEqual([]);
  const onDisk = new ChosenStore(disk());
  onDisk.choose(false);
  await onDisk.commit("a1", { mailboxes: { put: [INBOX] } });
  await onDisk.destroy();
  expect(await stored()).toEqual([]);
  expect(await onDisk.mailboxes("a1")).toEqual([]);
  const inMemory = new ChosenStore(disk());
  inMemory.choose(true);
  await inMemory.commit("a1", { mailboxes: { put: [INBOX] } });
  await inMemory.destroy();
  expect(await inMemory.mailboxes("a1")).toEqual([]);
});
