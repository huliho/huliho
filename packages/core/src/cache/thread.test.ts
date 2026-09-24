// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { JmapClient } from "../jmap/client";
import { z } from "../schema";
import { ACCOUNT, FakeJmap, at, email, json, mailbox } from "./fake-jmap";
import { PREVIEW_BATCH } from "./limits";
import { MemoryMailStore } from "./memory";
import { readThread } from "./thread";
import { queryWindow } from "./window";

interface Rig {
  server: FakeJmap;
  client: JmapClient;
  store: MemoryMailStore;
}

const idsSchema = z.array(z.string());

// `count` emails in one thread, e1 the oldest.
function serve(count: number): Rig {
  const server = new FakeJmap();
  server.putMailbox(mailbox("inbox", "inbox"));
  for (let index = 1; index <= count; index += 1) {
    server.addEmail(email(`e${String(index)}`, { threadId: "t1", receivedAt: at(index) }));
  }
  vi.stubGlobal("fetch", server.fetch);
  return { server, client: new JmapClient(ACCOUNT), store: new MemoryMailStore() };
}

// Whether a get asks for the headers with their previews or the states alone.
function kindOf(args: Record<string, unknown>): string {
  return Array.isArray(args["properties"]) && args["properties"].includes("preview")
    ? "headers"
    : "states";
}

// Every Email/get sent so far: its kind and how many ids it named.
function gets(server: FakeJmap): [string, number][] {
  return server
    .posted()
    .flat()
    .filter(([name]) => name === "Email/get")
    .map(([, args]) => [kindOf(args), idsSchema.parse(args["ids"] ?? []).length]);
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("a thread the list holds fetches the members' headers with their previews once", async () => {
  const { server, client, store } = serve(3);
  await queryWindow(client, store, "inbox", 0);
  server.requests.length = 0;
  const detail = await readThread(client, store, "t1");
  expect(detail?.thread.emailIds).toEqual(["e1", "e2", "e3"]);
  expect(Object.keys(detail?.emails ?? {})).toEqual(["e1", "e2", "e3"]);
  expect(detail?.emails["e1"]?.preview).toBe("Body of e1.");
  // The exemplar's header came with the list; the two others are asked for.
  expect(gets(server)).toEqual([["headers", 2]]);
  expect((await store.emails(ACCOUNT, ["e1"])).get("e1")?.preview).toBe("Body of e1.");
  expect(Object.keys((await store.threads(ACCOUNT, ["t1"])).get("t1")?.members ?? {})).toEqual([
    "e1",
    "e2",
    "e3",
  ]);
  server.requests.length = 0;
  await readThread(client, store, "t1");
  expect(server.requests).toHaveLength(0);
});

test("a thread the store lacks is fetched with the states an account without any starts from", async () => {
  const { server, client, store } = serve(2);
  const detail = await readThread(client, store, "t1");
  expect(detail?.thread.emailIds).toEqual(["e1", "e2"]);
  expect(detail?.thread.members["e2"]?.mailboxIds).toEqual({ inbox: true });
  expect(Object.keys(detail?.emails ?? {})).toEqual(["e1", "e2"]);
  const calls = server.posted();
  expect(calls[0]?.map(([callName, , id]) => `${callName}#${id}`)).toEqual([
    "Email/get#e0",
    "Thread/get#t0",
    "Thread/get#t",
  ]);
  expect(gets(server)).toEqual([
    ["states", 0],
    ["headers", 2],
  ]);
  expect(await store.state(ACCOUNT, "Email")).toBe(String(server.sequence));
  expect(await store.state(ACCOUNT, "Thread")).toBe(String(server.sequence));
  expect((await store.threads(ACCOUNT, ["t1"])).get("t1")?.emailIds).toEqual(["e1", "e2"]);
  expect(await readThread(client, store, "t9")).toBeNull();
});

test("a member whose preview is still empty is asked for again with the missing ones", async () => {
  const { server, client, store } = serve(3);
  await queryWindow(client, store, "inbox", 0);
  const exemplar = (await store.emails(ACCOUNT, ["e3"])).get("e3");
  if (exemplar === undefined) {
    throw new Error("the list holds no exemplar");
  }
  await store.commit(ACCOUNT, { emails: { put: [{ ...exemplar, preview: "" }] } });
  server.requests.length = 0;
  const detail = await readThread(client, store, "t1");
  expect(gets(server)).toEqual([["headers", 3]]);
  expect(detail?.emails["e3"]?.preview).toBe("Body of e3.");
});

test("the members are asked for in preview batches, whatever the server takes in one get", async () => {
  const { server, client, store } = serve(PREVIEW_BATCH * 2 + 5);
  await queryWindow(client, store, "inbox", 0);
  server.requests.length = 0;
  const detail = await readThread(client, store, "t1");
  expect(Object.keys(detail?.emails ?? {})).toHaveLength(PREVIEW_BATCH * 2 + 5);
  expect(gets(server)).toEqual([
    ["headers", PREVIEW_BATCH],
    ["headers", PREVIEW_BATCH],
    ["headers", 4],
  ]);
  expect(server.posted()).toHaveLength(1);
});

test("a refused fetch is the caller's and leaves the store as it was", async () => {
  const { server, client, store } = serve(3);
  await queryWindow(client, store, "inbox", 0);
  server.queue.push(json(409, { error: "still_stopped", cause: "connection" }));
  await expect(readThread(client, store, "t1")).rejects.toMatchObject({
    code: "stopped",
    stopCause: "connection",
  });
  expect((await store.emails(ACCOUNT, ["e1"])).size).toBe(0);
});
