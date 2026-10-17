// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// @vitest-environment node

import { MemoryMailStore } from "@huliho/core";
import type { MailStore, Mutation } from "@huliho/core";
import { ACCOUNT, FakeJmap, at, email } from "@huliho/core/testing";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { CHANGES_POLL_MS } from "./coordinator";
import type { CacheApi } from "./coordinator";
import { beginRig, endRig, holdLease, inMemory, ran, writable } from "./coordinator-rig";
import type { CacheMessage } from "./messages";
import { SET_FLUSH_MS } from "./writes";

interface Tab {
  posted: CacheMessage[];
  api: CacheApi;
}

// The thread t1 of e1 and e2 in Trash and e3 in the inbox.
function served(): FakeJmap {
  const server = new FakeJmap();
  server.putMailbox(writable("inbox", "inbox"));
  server.putMailbox(writable("archive", "archive"));
  server.putMailbox(writable("trash", "trash"));
  server.addEmail(email("e1", { threadId: "t1", receivedAt: at(1), mailboxIds: ["trash"] }));
  server.addEmail(email("e2", { threadId: "t1", receivedAt: at(2), mailboxIds: ["trash"] }));
  server.addEmail(email("e3", { receivedAt: at(3) }));
  server.recount();
  vi.stubGlobal("fetch", server.fetch);
  return server;
}

// A worker over `store` with one tab attached, its lists landed.
async function worker(store: MemoryMailStore): Promise<Tab> {
  const posted: CacheMessage[] = [];
  const api = inMemory(store, posted).api();
  await api.persisted();
  await api.attach({ accounts: [ACCOUNT], listedAt: Date.now(), watching: null, strict: false });
  await ran(0);
  await api.window(ACCOUNT, "trash", 0);
  await api.window(ACCOUNT, "inbox", 0);
  posted.length = 0;
  return { posted, api };
}

const FOREVER: Mutation[] = ["e1", "e2"].map((id) => ({
  type: "Email",
  id,
  threadId: "t1",
  destroy: true,
}));

function destroys(server: FakeJmap): unknown[] {
  return server
    .posted()
    .flat()
    .filter(([name]) => name === "Email/set")
    .map(([, args]) => args["destroy"]);
}

async function trashIds(store: MailStore): Promise<string[]> {
  return (await store.query(ACCOUNT, "trash"))?.pages[0]?.ids ?? [];
}

beforeEach(beginRig);
afterEach(endRig);

test("a delete forever waits held under a live lease: out of the list at once, nothing sent through the window and a poll", async () => {
  const server = served();
  const store = new MemoryMailStore();
  const tab = await worker(store);
  const letGo = await holdLease("tab-a");
  const answer = await tab.api.mutateMany(ACCOUNT, FOREVER, "tab-a");
  expect(answer).toMatchObject({ ok: true, value: { inverses: [], held: [1, 2] } });
  expect(await trashIds(store)).toEqual([]);
  expect(tab.posted.at(0)).toMatchObject({ kind: "changed", windows: ["trash"] });
  server.requests.length = 0;
  await ran(SET_FLUSH_MS);
  await ran(CHANGES_POLL_MS);
  expect(destroys(server)).toEqual([]);
  expect(server.emails.has("e1")).toBe(true);
  letGo();
});

test("release lets the held destroys go out with the next flush", async () => {
  const server = served();
  const store = new MemoryMailStore();
  const tab = await worker(store);
  const letGo = await holdLease("tab-a");
  await tab.api.mutateMany(ACCOUNT, FOREVER, "tab-a");
  await tab.api.release(ACCOUNT, [1, 2]);
  await ran(SET_FLUSH_MS);
  expect(destroys(server)).toEqual([["e1", "e2"]]);
  expect(server.emails.has("e1")).toBe(false);
  expect(await store.pending(ACCOUNT)).toEqual([]);
  letGo();
});

test("drop takes the held destroys back: the thread shows again and nothing goes out", async () => {
  const server = served();
  const store = new MemoryMailStore();
  const tab = await worker(store);
  const letGo = await holdLease("tab-a");
  await tab.api.mutateMany(ACCOUNT, FOREVER, "tab-a");
  tab.posted.length = 0;
  await tab.api.drop(ACCOUNT, [1, 2]);
  expect(await trashIds(store)).toEqual(["e2"]);
  expect(tab.posted).toEqual([expect.objectContaining({ kind: "changed" })]);
  await ran(SET_FLUSH_MS);
  await tab.api.release(ACCOUNT, [1, 2]);
  await ran(SET_FLUSH_MS);
  expect(destroys(server)).toEqual([]);
  letGo();
});

test("a held destroy whose tab holds no lease goes out at the next flush", async () => {
  const server = served();
  const store = new MemoryMailStore();
  const tab = await worker(store);
  await tab.api.mutateMany(ACCOUNT, FOREVER, "tab-gone");
  await ran(SET_FLUSH_MS);
  expect(destroys(server)).toEqual([["e1", "e2"]]);
});

test("a second worker over the same store leaves a live tab's held destroy alone, and lets a lapsed one go", async () => {
  const server = served();
  const store = new MemoryMailStore();
  const first = await worker(store);
  const letGo = await holdLease("tab-a");
  await first.api.mutateMany(ACCOUNT, FOREVER, "tab-a");
  await worker(store);
  await ran(SET_FLUSH_MS);
  expect(destroys(server)).toEqual([]);
  letGo();
  await ran(CHANGES_POLL_MS);
  expect(destroys(server)).toEqual([["e1", "e2"]]);
});

test("a change the mailbox refuses is left out of the batch, the rest logged with their inverses", async () => {
  served();
  const store = new MemoryMailStore();
  const tab = await worker(store);
  const archive: Mutation = {
    type: "Email",
    id: "e3",
    threadId: "t-e3",
    patch: { "mailboxIds/inbox": null, "mailboxIds/archive": true },
  };
  const nowhere: Mutation = { ...archive, patch: { "mailboxIds/inbox": null } };
  const answer = await tab.api.mutateMany(ACCOUNT, [archive, nowhere], "tab-a");
  expect(answer).toMatchObject({
    ok: true,
    value: {
      inverses: [{ id: "e3", patch: { "mailboxIds/inbox": true, "mailboxIds/archive": null } }],
      held: [],
      threads: ["t-e3"],
    },
  });
  expect(await store.pending(ACCOUNT)).toHaveLength(1);
});

test("a refusal names what each refused change did", async () => {
  const server = served();
  server.refused.set("e3", "forbidden");
  const store = new MemoryMailStore();
  const tab = await worker(store);
  await tab.api.mutateMany(
    ACCOUNT,
    [
      {
        type: "Email",
        id: "e3",
        threadId: "t-e3",
        patch: { "mailboxIds/inbox": null, "mailboxIds/archive": true },
      },
    ],
    "tab-a",
  );
  tab.posted.length = 0;
  await ran(SET_FLUSH_MS);
  expect(tab.posted.at(-1)).toEqual({
    kind: "refused",
    accountId: ACCOUNT,
    emailIds: ["e3"],
    verbs: ["archive"],
  });
});
