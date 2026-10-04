// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { BODY_PROPERTIES, BODY_VALUE_BYTES, BODY_VALUE_BYTES_LARGE } from "../jmap/body";
import type { EmailBodyPart } from "../jmap/body";
import { JmapClient } from "../jmap/client";
import { readBody } from "./bodies";
import { ACCOUNT, FakeJmap, UPSTREAM, at, email, json, mailbox } from "./fake-jmap";
import { BODY_CACHE_BYTES, BODY_CACHE_ROWS } from "./limits";
import { syncMailboxes } from "./mailboxes";
import { MemoryMailStore } from "./memory";
import { applyChanges } from "./poll";
import type { EmailBody } from "./store";
import { queryWindow } from "./window";

const SMALL = { large: false };
const LARGE = { large: true };
const OTHER = "acc-2";

// How deep the hostile tree of one test nests its parts.
const NESTED_PARTS = 1000;

// Two bodies of this weight pass the store's bound, one does not.
const HEAVY = Math.floor((BODY_CACHE_BYTES * 2) / 3);

interface Rig {
  server: FakeJmap;
  client: JmapClient;
  store: MemoryMailStore;
}

function serve(): Rig {
  const server = new FakeJmap();
  server.putMailbox(mailbox("inbox", "inbox"));
  server.addEmail(email("e1", { receivedAt: at(1) }));
  vi.stubGlobal("fetch", server.fetch);
  return { server, client: new JmapClient(ACCOUNT), store: new MemoryMailStore() };
}

function part(type: string): EmailBodyPart {
  return {
    partId: "1",
    blobId: "b1",
    size: 0,
    name: null,
    type,
    charset: null,
    disposition: null,
    cid: null,
    language: null,
    location: null,
  };
}

// A stored body of a chosen age and weight.
function stored(id: string, fetchedAt: number, bytes: number): EmailBody {
  return {
    id,
    bodyStructure: part("text/plain"),
    textBody: [part("text/plain")],
    htmlBody: [part("text/plain")],
    attachments: [],
    bodyValues: {},
    authentication: { status: "absent" },
    flowed: null,
    large: false,
    fetchedAt,
    bytes,
  };
}

// One Email/get answer as a server writes it, for a canned response.
function answered(list: readonly unknown[]): Response {
  return json(200, {
    methodResponses: [["Email/get", { accountId: UPSTREAM, state: "1", list, notFound: [] }, "b"]],
    sessionState: "s1",
  });
}

function failed(type: string): Response {
  return json(200, { methodResponses: [["error", { type }, "b"]], sessionState: "s1" });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("a message is asked once with both alternatives and their values, then served from the store", async () => {
  const { server, client, store } = serve();
  server.bodies.set("e1", { text: "Hello", html: "<p>Hello</p>" });
  const body = await readBody(client, store, "e1", SMALL);
  expect(server.posted()).toEqual([
    [
      [
        "Email/get",
        {
          accountId: UPSTREAM,
          ids: ["e1"],
          properties: [...BODY_PROPERTIES],
          fetchTextBodyValues: true,
          fetchHTMLBodyValues: true,
          maxBodyValueBytes: BODY_VALUE_BYTES,
        },
        "b",
      ],
    ],
  ]);
  expect(body?.textBody.map((one) => one.type)).toEqual(["text/plain"]);
  expect(body?.htmlBody.map((one) => one.type)).toEqual(["text/html"]);
  expect(body?.bodyValues).toEqual({
    "1": { value: "Hello", isEncodingProblem: false, isTruncated: false },
    "2": { value: "<p>Hello</p>", isEncodingProblem: false, isTruncated: false },
  });
  expect(body).toMatchObject({
    id: "e1",
    large: false,
    bytes: 17,
    flowed: null,
    authentication: { status: "absent" },
  });
  server.requests.length = 0;
  expect(await readBody(client, store, "e1", SMALL)).toEqual(body);
  // Nothing was cut, so the large ask reads the same row.
  expect(await readBody(client, store, "e1", LARGE)).toEqual(body);
  expect(server.requests).toHaveLength(0);
});

test("a body request the proxy's sanitizer could not cover is refused before any method runs", async () => {
  const { client } = serve();
  const ask = (args: Record<string, unknown>) =>
    client.request([
      { name: "Email/get", arguments: { accountId: UPSTREAM, ids: ["e1"], ...args }, id: "b" },
    ]);
  await expect(
    ask({ properties: ["textBody", "bodyValues"], fetchTextBodyValues: true }),
  ).rejects.toMatchObject({ code: "unavailable" });
  await expect(ask({ fetchAllBodyValues: true })).rejects.toMatchObject({ code: "unavailable" });
  await expect(ask({ properties: ["id", "subject"] })).resolves.toHaveLength(1);
});

test("a value the first ask cut short is asked again at the large cap when the caller says so", async () => {
  const { server, client, store } = serve();
  server.bodies.set("e1", { text: "0123456789" });
  server.bodyValueCap = 4;
  const cut = await readBody(client, store, "e1", SMALL);
  expect(cut?.bodyValues["1"]).toEqual({
    value: "0123",
    isEncodingProblem: false,
    isTruncated: true,
  });
  expect(cut?.bytes).toBe(4);
  server.requests.length = 0;
  expect(await readBody(client, store, "e1", SMALL)).toEqual(cut);
  expect(server.requests).toHaveLength(0);
  server.bodyValueCap = Number.POSITIVE_INFINITY;
  const whole = await readBody(client, store, "e1", LARGE);
  expect(whole?.bodyValues["1"]?.value).toBe("0123456789");
  expect(whole?.large).toBe(true);
  expect(server.posted().at(-1)?.[0]?.[1]).toMatchObject({
    maxBodyValueBytes: BODY_VALUE_BYTES_LARGE,
  });
  expect(await store.body(ACCOUNT, "e1")).toEqual(whole);
});

test("a body still cut at the large cap is kept as it came and not asked a third time", async () => {
  const { server, client, store } = serve();
  server.bodies.set("e1", { html: "<p>0123456789</p>" });
  server.bodyValueCap = 4;
  await readBody(client, store, "e1", SMALL);
  const large = await readBody(client, store, "e1", LARGE);
  expect(large).toMatchObject({ large: true });
  expect(large?.bodyValues["2"]?.isTruncated).toBe(true);
  server.requests.length = 0;
  expect(await readBody(client, store, "e1", LARGE)).toEqual(large);
  expect(await readBody(client, store, "e1", SMALL)).toEqual(large);
  expect(server.requests).toHaveLength(0);
});

test("only the topmost Authentication-Results header is read and a flowed text part is named", async () => {
  const { server, client, store } = serve();
  server.bodies.set("e1", {
    text: "one \ntwo",
    contentType: " text/plain; charset=utf-8;\r\n format=flowed; delsp=yes",
    authenticationResults: [
      " mx.example.net;\r\n dmarc=pass header.from=shop.example",
      " mx.example.net; dmarc=pass header.from=bank.example",
    ],
  });
  const body = await readBody(client, store, "e1", SMALL);
  expect(body?.flowed).toEqual({ delSp: true });
  expect(body?.authentication).toEqual({
    status: "parsed",
    results: {
      server: "mx.example.net",
      spf: "none",
      dkim: "none",
      dmarc: "pass",
      dmarcFrom: "shop.example",
    },
  });
});

test("a header the parser cannot read is kept as unparseable, never as absent", async () => {
  const { server, client, store } = serve();
  server.bodies.set("e1", { text: "x", authenticationResults: [" mx.example.net; no clause"] });
  const body = await readBody(client, store, "e1", SMALL);
  expect(body?.authentication).toEqual({ status: "unparseable" });
});

test("a part's children are never read, however deep a server nests them", async () => {
  const { server, client, store } = serve();
  await client.session();
  let tree: Record<string, unknown> = part("text/plain");
  for (let depth = 0; depth < NESTED_PARTS; depth += 1) {
    tree = { ...part("multipart/mixed"), subParts: [tree] };
  }
  server.queue.push(
    answered([
      {
        id: "e1",
        bodyStructure: tree,
        textBody: [],
        htmlBody: [],
        attachments: [],
        bodyValues: {},
        "header:Authentication-Results:asRaw:all": [],
        "header:Content-Type:asRaw": null,
      },
    ]),
  );
  const body = await readBody(client, store, "e1", SMALL);
  expect(body?.bodyStructure).toEqual(part("multipart/mixed"));
});

test("an answer for another email or without the body lists is never stored", async () => {
  const { server, client, store } = serve();
  await client.session();
  const other = {
    id: "e2",
    bodyStructure: part("text/plain"),
    textBody: [],
    htmlBody: [],
    attachments: [],
    bodyValues: {},
    "header:Authentication-Results:asRaw:all": [],
    "header:Content-Type:asRaw": null,
  };
  server.queue.push(answered([other]));
  expect(await readBody(client, store, "e1", SMALL)).toBeNull();
  server.queue.push(answered([{ id: "e1", bodyStructure: part("text/plain") }]));
  await expect(readBody(client, store, "e1", SMALL)).rejects.toThrow(/invalid/i);
  expect(await store.bodySizes()).toEqual([]);
});

test("an email the server does not have answers null and its stored body leaves", async () => {
  const { server, client, store } = serve();
  server.bodies.set("e1", { text: "0123456789" });
  server.bodyValueCap = 4;
  await readBody(client, store, "e1", SMALL);
  server.destroyEmail("e1");
  expect(await readBody(client, store, "e1", LARGE)).toBeNull();
  expect(await store.body(ACCOUNT, "e1")).toBeNull();
  expect(await readBody(client, store, "e9", SMALL)).toBeNull();
});

test("an email another client destroyed takes its stored body with it at the next poll", async () => {
  const { server, client, store } = serve();
  server.addEmail(email("e2", { receivedAt: at(2) }));
  await syncMailboxes(client, store);
  await queryWindow(client, store, "inbox", 0);
  await readBody(client, store, "e1", SMALL);
  await readBody(client, store, "e2", SMALL);
  server.destroyEmail("e1");
  await applyChanges(client, store, ["inbox"]);
  expect(await store.body(ACCOUNT, "e1")).toBeNull();
  expect(await store.body(ACCOUNT, "e2")).not.toBeNull();
});

test("a mail server out of reach reads as unavailable and any other refusal is the caller's (RFC 8620 section 3.6.2)", async () => {
  const { server, client, store } = serve();
  await client.session();
  server.queue.push(failed("serverUnavailable"));
  await expect(readBody(client, store, "e1", SMALL)).rejects.toMatchObject({
    name: "JmapError",
    code: "unavailable",
  });
  server.queue.push(failed("invalidArguments"));
  await expect(readBody(client, store, "e1", SMALL)).rejects.toMatchObject({
    name: "MethodFailure",
    type: "invalidArguments",
  });
  const problem = { type: "urn:ietf:params:jmap:error:limit", limit: "maxConcurrentRequests" };
  server.queue.push(json(400, problem, "application/problem+json"));
  await expect(readBody(client, store, "e1", SMALL)).rejects.toMatchObject({ code: "limit" });
  expect(await store.body(ACCOUNT, "e1")).toBeNull();
  expect(await readBody(client, store, "e1", SMALL)).toMatchObject({ id: "e1" });
});

test("past the row bound the body fetched longest ago leaves, whichever account holds it", async () => {
  const { client, store } = serve();
  const held = Array.from({ length: BODY_CACHE_ROWS - 1 }, (_, index) =>
    stored(`old-${String(index)}`, index + 2, 1),
  );
  await store.commit(ACCOUNT, { bodies: { put: held } });
  await store.commit(OTHER, { bodies: { put: [stored("oldest", 1, 1)] } });
  await readBody(client, store, "e1", SMALL);
  expect(await store.body(OTHER, "oldest")).toBeNull();
  expect(await store.bodySizes()).toHaveLength(BODY_CACHE_ROWS);
  expect(await store.body(ACCOUNT, "old-0")).not.toBeNull();
  expect(await store.body(ACCOUNT, "e1")).not.toBeNull();
});

test("past the byte bound the oldest bodies leave until the rest fits", async () => {
  const { client, store } = serve();
  await store.commit(OTHER, { bodies: { put: [stored("first", 1, HEAVY)] } });
  await store.commit(ACCOUNT, { bodies: { put: [stored("second", 2, HEAVY)] } });
  await readBody(client, store, "e1", SMALL);
  expect(await store.body(OTHER, "first")).toBeNull();
  expect((await store.bodySizes()).map((size) => size.id)).toEqual(["second", "e1"]);
});
