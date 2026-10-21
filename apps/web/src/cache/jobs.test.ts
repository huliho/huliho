// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// @vitest-environment node

import { MemoryMailStore } from "@huliho/core";
import { ACCOUNT, FakeJmap, at, email } from "@huliho/core/testing";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import type { CacheApi } from "./coordinator";
import { beginRig, endRig, inMemory, ran, writable } from "./coordinator-rig";
import type { CacheMessage } from "./messages";

// The server takes two emails per set, so five unread ones take three rounds.
const SET_LIMIT = 2;
const UNREAD = 5;

// A quota of 10 GiB, a quarter of it used.
const GIB = 1024 ** 3;
const LIMIT_OCTETS = 10 * GIB;
const USED_OCTETS = LIMIT_OCTETS / 4;

const MARK_READ = { kind: "markRead", mailboxId: "inbox" } as const;

interface Rig {
  server: FakeJmap;
  posted: CacheMessage[];
  api: CacheApi;
}

// An inbox of five unread emails on a server that takes two per set.
async function served(knobs: (server: FakeJmap) => void = () => undefined): Promise<Rig> {
  const server = new FakeJmap();
  server.maxObjectsInSet = SET_LIMIT;
  server.putMailbox(writable("inbox", "inbox"));
  for (let index = 1; index <= UNREAD; index += 1) {
    server.addEmail(email(`e${String(index)}`, { receivedAt: at(index) }));
  }
  server.recount();
  knobs(server);
  vi.stubGlobal("fetch", server.fetch);
  const posted: CacheMessage[] = [];
  const api = inMemory(new MemoryMailStore(), posted).api();
  await api.persisted();
  await api.attach({ accounts: [ACCOUNT], listedAt: Date.now(), watching: null, strict: false });
  await ran(0);
  posted.length = 0;
  server.requests.length = 0;
  return { server, posted, api };
}

function progress(posted: readonly CacheMessage[]): unknown[] {
  return posted.flatMap((message) =>
    message.kind === "job"
      ? [[message.progress.done, message.progress.total, message.progress.state]]
      : [],
  );
}

function methods(server: FakeJmap): string[] {
  return server
    .posted()
    .flat()
    .map(([name]) => name);
}

beforeEach(beginRig);
afterEach(endRig);

test("a job runs round by round and tells every tab, from its start on; the account polls once it ends", async () => {
  const { server, posted, api } = await served();
  expect(await api.job(ACCOUNT, MARK_READ)).toEqual({ ok: true, value: undefined });
  await ran(0);
  expect(progress(posted)).toEqual([
    [0, null, "running"],
    [2, UNREAD, "running"],
    [4, UNREAD, "running"],
    [UNREAD, UNREAD, "running"],
    [UNREAD, UNREAD, "done"],
  ]);
  expect([...server.emails.values()].every((row) => "$seen" in row.keywords)).toBe(true);
  expect(posted.filter((message) => message.kind === "changed").length).toBeGreaterThan(0);
  // Three rounds write and carry the mailbox changes; a fourth finds
  // nothing; the poll after the job asks the changes once more.
  expect(methods(server).slice(-4)).toEqual([
    "Email/query",
    "Mailbox/changes",
    "Mailbox/get",
    "Mailbox/get",
  ]);
});

test("a job asked while one of the account runs is left alone", async () => {
  const { posted, api } = await served();
  await api.job(ACCOUNT, MARK_READ);
  await api.job(ACCOUNT, { kind: "empty", mailboxId: "inbox" });
  await ran(0);
  const kinds = posted.flatMap((message) =>
    message.kind === "job" ? [message.progress.job.kind] : [],
  );
  expect(new Set(kinds)).toEqual(new Set(["markRead"]));
});

test("cancel ends the job after the round that runs", async () => {
  const { posted, api } = await served();
  await api.job(ACCOUNT, MARK_READ);
  await api.cancelJob(ACCOUNT);
  await ran(0);
  expect(progress(posted)).toEqual([
    [0, null, "running"],
    [2, UNREAD, "canceled"],
  ]);
});

test("a round the server refuses ends the job as failed with what came before kept, and the account still polls", async () => {
  const { server, posted, api } = await served((fake) => {
    fake.refused.set("e5", "forbidden");
    fake.refused.set("e4", "forbidden");
  });
  await api.job(ACCOUNT, MARK_READ);
  await ran(0);
  expect(progress(posted)).toEqual([
    [0, null, "running"],
    [0, null, "failed"],
  ]);
  expect(methods(server).filter((name) => name === "Mailbox/changes")).toHaveLength(2);
  expect(methods(server).at(-3)).toBe("Mailbox/changes");
});

test("the quota reads the fullest quota of the account, null without one", async () => {
  const { api } = await served();
  expect(await api.quota(ACCOUNT)).toEqual({ ok: true, value: null });
  const quotaRig = await served((fake) => {
    fake.quota = true;
    fake.putQuota({
      id: "q1",
      resourceType: "octets",
      used: USED_OCTETS,
      hardLimit: LIMIT_OCTETS,
      scope: "account",
      name: "mail",
      types: ["Email"],
    });
  });
  await quotaRig.api.attach({
    accounts: [ACCOUNT],
    listedAt: Date.now(),
    watching: null,
    strict: false,
  });
  expect(await quotaRig.api.quota(ACCOUNT)).toEqual({
    ok: true,
    value: { unit: "octets", used: USED_OCTETS, limit: LIMIT_OCTETS, share: 0.25, warn: false },
  });
});
