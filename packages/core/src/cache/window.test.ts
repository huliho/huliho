// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { JmapClient } from "../jmap/client";
import { z } from "../schema";
import { ACCOUNT, FakeJmap, at, email, json, mailbox } from "./fake-jmap";
import { WINDOW_SIZE } from "./limits";
import { MemoryMailStore } from "./memory";
import { queryWindow } from "./window";

interface Rig {
  server: FakeJmap;
  client: JmapClient;
  store: MemoryMailStore;
}

// `count` emails in threads of `size`, the newest last; e1 is the oldest.
function serve(count: number, size = 1): Rig {
  const server = new FakeJmap();
  server.putMailbox(mailbox("inbox", "inbox"));
  for (let index = 1; index <= count; index += 1) {
    const thread = `t${String(Math.ceil(index / size))}`;
    server.addEmail(email(`e${String(index)}`, { threadId: thread, receivedAt: at(index) }));
  }
  vi.stubGlobal("fetch", server.fetch);
  return { server, client: new JmapClient(ACCOUNT), store: new MemoryMailStore() };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("the first page comes in one round trip with the headers, the threads and every member's state", async () => {
  const { server, client, store } = serve(230, 2);
  const page = await queryWindow(client, store, "inbox", 0);
  const calls = server.posted();
  expect(calls).toHaveLength(1);
  expect(calls[0]?.map(([name, , id]) => `${name}#${id}`)).toEqual([
    "Email/get#e0",
    "Thread/get#t0",
    "Email/query#q",
    "Email/get#h",
    "Thread/get#t",
    "Email/get#m",
  ]);
  expect(calls[0]?.[2]?.[1]).toMatchObject({
    filter: { inMailbox: "inbox" },
    sort: [{ property: "receivedAt", isAscending: false }],
    position: 0,
    limit: WINDOW_SIZE,
    calculateTotal: true,
    collapseThreads: true,
  });
  expect(page.ids).toHaveLength(WINDOW_SIZE);
  expect(page.ids[0]).toBe("e230");
  expect(page.total).toBe(115);
  expect(page.pending).toBe(0);
  const exemplar = page.emails["e230"];
  expect(exemplar?.subject).toBe("Message e230");
  const thread = page.threads["t115"];
  expect(thread?.emailIds).toEqual(["e229", "e230"]);
  expect(Object.keys(thread?.members ?? {})).toEqual(["e229", "e230"]);
  expect(await store.state(ACCOUNT, "Email")).toBe(String(server.sequence));
  expect(await store.state(ACCOUNT, "Thread")).toBe(String(server.sequence));
});

test("a page the store holds costs no request", async () => {
  const { server, client, store } = serve(12);
  await queryWindow(client, store, "inbox", 0);
  const posted = server.requests.length;
  const again = await queryWindow(client, store, "inbox", 0);
  expect(server.requests).toHaveLength(posted);
  expect(again.ids).toHaveLength(12);
  expect(again.total).toBe(12);
});

test("the next page is asked by anchor on the last row of the page before it", async () => {
  const { server, client, store } = serve(230, 2);
  const first = await queryWindow(client, store, "inbox", 0);
  const second = await queryWindow(client, store, "inbox", 1);
  const calls = server.posted().at(-1);
  expect(calls?.map(([name]) => name)).toEqual([
    "Email/query",
    "Email/get",
    "Thread/get",
    "Email/get",
  ]);
  expect(calls?.[0]?.[1]).toMatchObject({
    anchor: first.ids.at(-1),
    anchorOffset: 1,
    calculateTotal: false,
  });
  expect(second.ids).toHaveLength(15);
  expect(second.ids[0]).toBe("e30");
  expect(second.total).toBe(115);
});

test("an anchor that left the list falls back to a position", async () => {
  const { server, client, store } = serve(120);
  const first = await queryWindow(client, store, "inbox", 0);
  const last = first.ids.at(-1) ?? "";
  server.destroyEmail(last);
  const second = await queryWindow(client, store, "inbox", 1);
  const attempts = server.posted().slice(1);
  expect(attempts).toHaveLength(2);
  expect(attempts[0]?.[0]?.[1]).toMatchObject({ anchor: last });
  expect(attempts[1]?.[0]?.[1]).toMatchObject({ position: WINDOW_SIZE });
  expect(second.ids).toHaveLength(19);
});

test("a lowered limit fills the page in follow-up chunks by anchor", async () => {
  const { server, client, store } = serve(150);
  server.queryLimit = 40;
  const page = await queryWindow(client, store, "inbox", 0);
  expect(page.ids).toHaveLength(WINDOW_SIZE);
  expect(page.ids[0]).toBe("e150");
  expect(page.ids.at(-1)).toBe("e51");
  expect(page.total).toBe(150);
  const chunks = server.posted();
  expect(chunks).toHaveLength(3);
  expect(chunks[1]?.[0]?.[1]).toMatchObject({ anchor: "e111", anchorOffset: 1, limit: 60 });
  expect(chunks[2]?.[0]?.[1]).toMatchObject({ anchor: "e71", anchorOffset: 1, limit: 20 });
});

test("a members answer too large is fetched in the chunks the server takes", async () => {
  const { server, client, store } = serve(200, 2);
  server.maxObjectsInGet = 150;
  const page = await queryWindow(client, store, "inbox", 0);
  expect(page.ids).toHaveLength(WINDOW_SIZE);
  const posted = server.posted();
  expect(posted).toHaveLength(2);
  const idsSchema = z.array(z.string());
  expect(posted[1]?.map(([name, args]) => [name, idsSchema.parse(args["ids"]).length])).toEqual([
    ["Email/get", 150],
    ["Email/get", 50],
  ]);
  expect(Object.keys(page.threads["t100"]?.members ?? {})).toEqual(["e199", "e200"]);
  expect(Object.keys(page.threads["t1"]?.members ?? {})).toEqual(["e1", "e2"]);
});

test("ids the get does not find leave the page and objects the query did not name are ignored", async () => {
  const { server, client, store } = serve(3);
  await client.session();
  const stray = email("e9", { receivedAt: at(9) });
  server.queue.push(
    json(200, {
      methodResponses: [
        ["Email/get", { state: "3", list: [], notFound: [] }, "e0"],
        ["Thread/get", { state: "3", list: [], notFound: [] }, "t0"],
        ["Email/query", { queryState: "3", position: 0, ids: ["e3", "e2"], total: 2 }, "q"],
        [
          "Email/get",
          { state: "3", list: [server.emails.get("e3"), stray], notFound: ["e2"] },
          "h",
        ],
        ["Thread/get", { state: "3", list: [{ id: "t-e3", emailIds: ["e3"] }], notFound: [] }, "t"],
        [
          "Email/get",
          {
            state: "3",
            list: [{ id: "e3", threadId: "t-e3", mailboxIds: {}, keywords: {} }],
            notFound: [],
          },
          "m",
        ],
      ],
      sessionState: "s1",
    }),
  );
  const page = await queryWindow(client, store, "inbox", 0);
  expect(page.ids).toEqual(["e3"]);
  expect(Object.keys(page.emails)).toEqual(["e3"]);
  expect((await store.emails(ACCOUNT, ["e9"])).size).toBe(0);
});

test("a refused query is the caller's", async () => {
  const { client, store } = serve(3);
  await expect(queryWindow(client, store, "missing", 0)).resolves.toMatchObject({ ids: [] });
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue(json(409, { error: "still_stopped", cause: "connection" })),
  );
  await expect(queryWindow(new JmapClient(ACCOUNT), store, "inbox", 0)).rejects.toMatchObject({
    code: "stopped",
    stopCause: "connection",
  });
});
