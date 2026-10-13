// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { JmapClient } from "../jmap/client";
import { QUOTA_CAPABILITY } from "../jmap/schemas";
import { ACCOUNT, FakeJmap, json, mailbox, quota } from "./fake-jmap";
import { GIB } from "./fake-quota";
import { syncMailboxes } from "./mailboxes";
import { MemoryMailStore } from "./memory";
import { applyChanges } from "./poll";
import { QUOTA_WARN_RATIO, pickQuota, readQuota } from "./quota";

interface Rig {
  server: FakeJmap;
  client: JmapClient;
  store: MemoryMailStore;
}

function serve(knobs: (server: FakeJmap) => void = () => undefined): Rig {
  const server = new FakeJmap();
  server.putMailbox(mailbox("inbox", "inbox"));
  server.quota = true;
  server.putQuota(quota("q1", { used: GIB / 4 }));
  knobs(server);
  vi.stubGlobal("fetch", server.fetch);
  return { server, client: new JmapClient(ACCOUNT), store: new MemoryMailStore() };
}

// The method calls of the last request, by name and call id.
function lastCalls(server: FakeJmap): string[] {
  return (server.posted().at(-1) ?? []).map(([name, , id]) => `${name}#${id}`);
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("the bar shows the fullest octets quota of the account's mail, capped at full", () => {
  const list = [
    quota("a", { used: 1, hardLimit: 10 }),
    quota("b", { used: 7, hardLimit: 10, types: ["Mail"] }),
    quota("c", { used: 9, hardLimit: 10, scope: "domain" }),
    quota("d", { used: 9, hardLimit: 10, types: ["Calendar"] }),
    quota("e", { used: 9, hardLimit: 10, resourceType: "count" }),
    quota("f", { used: 5, hardLimit: 0 }),
  ];
  expect(pickQuota(list)).toEqual({ unit: "octets", used: 7, limit: 10, share: 0.7, warn: false });
  expect(pickQuota([quota("g", { used: 12, hardLimit: 10 })])).toMatchObject({
    share: 1,
    warn: true,
  });
});

test("the warn rule is the server's warn limit where it names one, else 90% of the hard limit", () => {
  const limit = 1000;
  const below = QUOTA_WARN_RATIO * limit - 1;
  expect(pickQuota([quota("a", { used: below, hardLimit: limit })])?.warn).toBe(false);
  expect(pickQuota([quota("a", { used: below + 1, hardLimit: limit })])?.warn).toBe(true);
  expect(pickQuota([quota("a", { used: 500, hardLimit: limit, warnLimit: 400 })])?.warn).toBe(true);
  expect(pickQuota([quota("a", { used: 950, hardLimit: limit, warnLimit: 990 })])?.warn).toBe(
    false,
  );
});

test("a count quota alone reads in messages and an account without a quota of its mail shows none", () => {
  const counted = quota("m", { resourceType: "count", used: 40, hardLimit: 100 });
  expect(pickQuota([counted])).toEqual({
    unit: "count",
    used: 40,
    limit: 100,
    share: 0.4,
    warn: false,
  });
  expect(pickQuota([counted, quota("o", { used: 1, hardLimit: 10 })])?.unit).toBe("octets");
  expect(pickQuota([])).toBeNull();
  expect(pickQuota([quota("x", { scope: "global" })])).toBeNull();
});

test("the first mailbox pass asks every quota beside the tree and keeps them under their state (RFC 9425 section 4.2)", async () => {
  const { server, client, store } = serve();
  expect((await client.session()).using).toContain(QUOTA_CAPABILITY);
  expect(await syncMailboxes(client, store)).toBe(true);
  expect(lastCalls(server)).toEqual(["Mailbox/get#m", "Quota/get#q:Quota"]);
  expect(server.posted().at(-1)?.[1]?.[1]).toEqual({ accountId: "u1", ids: null });
  expect(await store.quota(ACCOUNT)).toEqual({
    state: String(server.sequence),
    list: [quota("q1", { used: GIB / 4 })],
  });
  expect(await readQuota(store, ACCOUNT)).toEqual({
    unit: "octets",
    used: GIB / 4,
    limit: GIB,
    share: 0.25,
    warn: false,
  });
});

test("a later pass follows the quota's changes with the mailbox changes and a moved quota reads as a changed tree", async () => {
  const { server, client, store } = serve();
  await syncMailboxes(client, store);
  expect(await syncMailboxes(client, store)).toBe(false);
  expect(lastCalls(server)).toEqual([
    "Mailbox/changes#c:Mailbox",
    "Mailbox/get#n:Mailbox",
    "Mailbox/get#u:Mailbox",
    "Quota/changes#c:Quota",
    "Quota/get#n:Quota",
    "Quota/get#u:Quota",
  ]);
  server.putQuota(quota("q1", { used: GIB }));
  server.putQuota(quota("q2", { resourceType: "count", used: 3, hardLimit: 10 }));
  const applied = await applyChanges(client, store, []);
  expect(applied.mailboxes).toBe(true);
  expect((await store.quota(ACCOUNT))?.list.map((row) => [row.id, row.used])).toEqual([
    ["q1", GIB],
    ["q2", 3],
  ]);
  expect((await readQuota(store, ACCOUNT))?.warn).toBe(true);
  server.removeQuota("q2");
  await syncMailboxes(client, store);
  expect((await store.quota(ACCOUNT))?.list.map((row) => row.id)).toEqual(["q1"]);
});

test("a quota state that moves with the list unchanged is held under the new state and reads as an unchanged tree", async () => {
  const { server, client, store } = serve();
  await syncMailboxes(client, store);
  const before = await store.quota(ACCOUNT);
  server.putQuota(quota("q1", { used: GIB / 4 }));
  const applied = await applyChanges(client, store, []);
  expect(lastCalls(server)).toContain("Quota/changes#c:Quota");
  expect(applied.mailboxes).toBe(false);
  const after = await store.quota(ACCOUNT);
  expect(after).toEqual({ state: String(server.sequence), list: [quota("q1", { used: GIB / 4 })] });
  expect(after?.state).not.toBe(before?.state);
});

test("a state the server cannot follow has the list asked again (RFC 8620 section 5.2)", async () => {
  const { server, client, store } = serve();
  await syncMailboxes(client, store);
  server.putQuota(quota("q1", { used: 5 }));
  server.forget();
  await syncMailboxes(client, store);
  expect(lastCalls(server)).toEqual(["Mailbox/get#m", "Quota/get#q:Quota"]);
  await store.commit(ACCOUNT, { quota: { state: "stale", list: [] } });
  server.requests.length = 0;
  await applyChanges(client, store, []);
  expect(server.posted().map((calls) => calls.at(-1)?.[0])).toEqual(["Quota/get", "Quota/get"]);
  expect((await store.quota(ACCOUNT))?.list[0]?.used).toBe(5);
});

test("an account that does not list the capability is never asked and holds no quota", async () => {
  const { server, client, store } = serve((knobs) => {
    knobs.quotaAccount = false;
  });
  const session = await client.session();
  expect(session.quota).toBe(false);
  expect(session.using).toContain(QUOTA_CAPABILITY);
  await syncMailboxes(client, store);
  expect(lastCalls(server)).toEqual(["Mailbox/get#m"]);
  expect(await store.quota(ACCOUNT)).toBeNull();
  expect(await readQuota(store, ACCOUNT)).toBeNull();
});

test("a server without the capability is never asked, and a quota held from before is dropped", async () => {
  const { server, client, store } = serve((knobs) => {
    knobs.quota = false;
  });
  expect((await client.session()).using).not.toContain(QUOTA_CAPABILITY);
  await store.commit(ACCOUNT, { quota: { state: "1", list: [quota("q1")] } });
  await syncMailboxes(client, store);
  expect(lastCalls(server)).toEqual(["Mailbox/get#m"]);
  expect(await store.quota(ACCOUNT)).toBeNull();
});

test("an account the server says answers no quota holds none (RFC 8620 section 3.6.2)", async () => {
  const { server, client, store } = serve();
  await syncMailboxes(client, store);
  // The session object the client holds still lists quota on the account.
  server.quotaAccount = false;
  await syncMailboxes(client, store);
  expect(await store.quota(ACCOUNT)).toBeNull();
});

test("a quota answer the client cannot read keeps what it holds and the tree lands", async () => {
  const { server, client, store } = serve();
  await syncMailboxes(client, store);
  const held = await store.quota(ACCOUNT);
  await store.commit(ACCOUNT, { states: { Mailbox: null } });
  server.putMailbox({ ...mailbox("inbox", "inbox"), name: "Post" });
  server.queue.push(
    json(200, {
      methodResponses: [
        ["Mailbox/get", { state: "9", list: [server.mailboxes.get("inbox")], notFound: [] }, "m"],
        ["Quota/get", { state: "9", list: [{ id: "q1", used: -1 }], notFound: [] }, "q:Quota"],
      ],
      sessionState: "s1",
    }),
  );
  await syncMailboxes(client, store);
  expect((await store.mailboxes(ACCOUNT))[0]?.name).toBe("Post");
  expect(await store.quota(ACCOUNT)).toEqual(held);
  server.queue.push(
    json(200, {
      methodResponses: [
        ["Mailbox/get", { state: "9", list: [], notFound: [] }, "m"],
        ["error", { type: "serverFail" }, "q:Quota"],
      ],
      sessionState: "s1",
    }),
  );
  await store.commit(ACCOUNT, { states: { Mailbox: null } });
  await syncMailboxes(client, store);
  expect(await store.quota(ACCOUNT)).toEqual(held);
});
