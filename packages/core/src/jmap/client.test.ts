// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { ACCOUNT, FakeJmap, UPSTREAM, json, mailbox } from "../cache/fake-jmap";
import { z } from "../schema";
import { answer, outcome } from "./calls";
import { JmapClient, JmapError, MethodFailure } from "./client";
import { CORE_CAPABILITY, HULIHO_CAPABILITY, MAIL_CAPABILITY } from "./schemas";

const listSchema = z.object({ list: z.array(z.unknown()) });

function serve(): FakeJmap {
  const server = new FakeJmap();
  server.putMailbox(mailbox("inbox", "inbox"));
  vi.stubGlobal("fetch", server.fetch);
  return server;
}

const GET_ALL = { name: "Mailbox/get", arguments: { accountId: UPSTREAM, ids: null }, id: "a" };

afterEach(() => {
  vi.unstubAllGlobals();
});

test("the session comes from the account's route and names the upstream account, the endpoint and the capabilities", async () => {
  const server = serve();
  const session = await new JmapClient(ACCOUNT).session();
  expect(server.requests[0]?.url).toBe("/api/jmap/acc-1/session");
  expect(session).toEqual({
    accountId: UPSTREAM,
    apiUrl: "/api/jmap/acc-1",
    using: [CORE_CAPABILITY, MAIL_CAPABILITY, HULIHO_CAPABILITY],
    firstSync: true,
    maxCallsInRequest: 16,
    maxObjectsInGet: 500,
    state: "s1",
  });
});

test("a native account without the vendor capability opts into core and mail alone", async () => {
  const server = serve();
  server.vendor = false;
  const session = await new JmapClient(ACCOUNT).session();
  expect(session.using).toEqual([CORE_CAPABILITY, MAIL_CAPABILITY]);
  expect(session.firstSync).toBe(false);
});

test("a session naming no mail account, an endpoint off this origin or no core limits is refused", async () => {
  const server = serve();
  const object = server.session();
  server.queue.push(json(200, { ...object, primaryAccounts: {} }));
  await expect(new JmapClient(ACCOUNT).session()).rejects.toMatchObject({ code: "upstream" });
  server.queue.push(json(200, { ...object, apiUrl: "https://mail.example.test/jmap" }));
  await expect(new JmapClient(ACCOUNT).session()).rejects.toThrow(/invalid/i);
  server.queue.push(json(200, { ...object, capabilities: { [MAIL_CAPABILITY]: {} } }));
  await expect(new JmapClient(ACCOUNT).session()).rejects.toThrow(/invalid/i);
});

test("a request posts the calls to the endpoint as JSON with the header and answers in order", async () => {
  const server = serve();
  const client = new JmapClient(ACCOUNT);
  const responses = await client.request([GET_ALL, { ...GET_ALL, id: "b" }]);
  const posted = server.requests[1];
  expect(posted?.method).toBe("POST");
  expect(posted?.url).toBe("/api/jmap/acc-1");
  expect(posted?.headers.get("content-type")).toBe("application/json");
  expect(posted?.headers.get("x-requested-with")).toBe("huliho");
  expect(posted?.body).toEqual({
    using: [CORE_CAPABILITY, MAIL_CAPABILITY, HULIHO_CAPABILITY],
    methodCalls: [
      ["Mailbox/get", { accountId: UPSTREAM, ids: null }, "a"],
      ["Mailbox/get", { accountId: UPSTREAM, ids: null }, "b"],
    ],
  });
  expect(responses.map(([name, , id]) => [name, id])).toEqual([
    ["Mailbox/get", "a"],
    ["Mailbox/get", "b"],
  ]);
  expect(answer(responses, "b", listSchema).list).toHaveLength(1);
});

test("a moved sessionState has the next call fetch the session again", async () => {
  const server = serve();
  const client = new JmapClient(ACCOUNT);
  await client.request([GET_ALL]);
  server.sessionState = "s2";
  await client.request([GET_ALL]);
  await client.request([GET_ALL]);
  const sessions = server.requests.filter((request) => request.url.endsWith("/session"));
  expect(sessions).toHaveLength(2);
  expect((await client.session()).state).toBe("s2");
});

test("more calls than the session allows are refused before anything is sent", async () => {
  const server = serve();
  server.maxCallsInRequest = 1;
  const client = new JmapClient(ACCOUNT);
  await expect(client.request([GET_ALL, { ...GET_ALL, id: "b" }])).rejects.toMatchObject({
    code: "limit",
    limit: "maxCallsInRequest",
  });
  expect(server.requests.filter((request) => request.method === "POST")).toHaveLength(0);
});

test.each([
  [409, { error: "still_stopped", cause: "credentials" }, "stopped", "credentials"],
  [409, { error: "still_stopped", cause: "connection" }, "stopped", "connection"],
  [409, { error: "still_stopped" }, "unavailable", null],
  [401, { error: "unauthenticated" }, "unauthenticated", null],
  [401, { error: "upstream_credentials" }, "credentials", null],
  [404, { error: "not_found" }, "not_found", null],
  [502, { error: "upstream_failed" }, "upstream", null],
  [502, { error: "upstream_unreachable" }, "upstream", null],
  [400, { error: "upstream_unsupported" }, "upstream", null],
  [400, { error: "upstream_insecure" }, "upstream", null],
  [500, { error: "internal" }, "unavailable", null],
])("a %i answer with %j reads as %s", async (status, body, code, stopCause) => {
  const server = serve();
  server.queue.push(json(status, body));
  const failure = await new JmapClient(ACCOUNT).session().catch((error: unknown) => error);
  expect(failure).toBeInstanceOf(JmapError);
  expect(failure).toMatchObject({ code, stopCause, limit: null });
});

test("the limit problem, the body layer's 413 and an unreachable server read as they are", async () => {
  const server = serve();
  const client = new JmapClient(ACCOUNT);
  const problem = { type: "urn:ietf:params:jmap:error:limit", limit: "maxConcurrentRequests" };
  server.queue.push(json(400, problem, "application/problem+json"));
  await expect(client.request([GET_ALL])).rejects.toMatchObject({
    code: "limit",
    limit: "maxConcurrentRequests",
  });
  server.queue.push(new Response("length limit exceeded", { status: 413 }));
  await expect(client.request([GET_ALL])).rejects.toMatchObject({
    code: "limit",
    limit: "maxSizeRequest",
  });
  server.queue.push(new Response("<html>bad gateway</html>", { status: 503 }));
  await expect(client.request([GET_ALL])).rejects.toMatchObject({ code: "unavailable" });
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("down")));
  await expect(client.request([GET_ALL])).rejects.toMatchObject({ code: "unavailable" });
});

test("an answer that is no Response object is refused at the boundary", async () => {
  const server = serve();
  const client = new JmapClient(ACCOUNT);
  await client.session();
  server.queue.push(json(200, { methodResponses: [["Mailbox/get", {}]], sessionState: "s1" }));
  await expect(client.request([GET_ALL])).rejects.toThrow(/methodResponses/);
});

test("a method that answered an error is read as such and a missing response is a broken server", async () => {
  serve();
  const client = new JmapClient(ACCOUNT);
  const responses = await client.request([
    { name: "Email/get", arguments: { accountId: UPSTREAM, ids: null }, id: "a" },
  ]);
  expect(outcome(responses, "a", listSchema)).toEqual({ status: "error", type: "requestTooLarge" });
  const failure = (() => {
    try {
      answer(responses, "a", listSchema);
      return null;
    } catch (error: unknown) {
      return error;
    }
  })();
  expect(failure).toBeInstanceOf(MethodFailure);
  expect(failure).toMatchObject({ type: "requestTooLarge", call: "a" });
  expect(() => answer(responses, "b", listSchema)).toThrow("no response to call b");
});
