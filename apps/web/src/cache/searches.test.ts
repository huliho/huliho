// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// @vitest-environment node

import { searchKeyOf } from "@huliho/core";
import { ACCOUNT, at } from "@huliho/core/testing";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { attached, beginRig, drained, endRig } from "./coordinator-rig";

// One message of the three the rig serves names "e2" in its subject.
const KEY = searchKeyOf({ q: "e2" });
const CALL = { locale: "en", again: false, ticket: "t1" };

beforeEach(beginRig);
afterEach(endRig);

async function waiting(count: () => number, expected: number): Promise<void> {
  await vi.waitFor(() => {
    expect(count()).toBe(expected);
  });
}

test("a search answers its page, then answers it again from memory with no request", async () => {
  const { server, tab } = await attached(3);
  const first = await tab.search(ACCOUNT, KEY, 0, CALL);
  expect(first).toMatchObject({ ok: true, value: { total: 1, rows: [{ id: "e2" }] } });
  const sent = server.posted().length;
  await expect(tab.search(ACCOUNT, KEY, 0, { ...CALL, ticket: "t2" })).resolves.toEqual(first);
  expect(server.posted()).toHaveLength(sent);
  await tab.search(ACCOUNT, KEY, 0, { ...CALL, again: true, ticket: "t3" });
  expect(server.posted()).toHaveLength(sent + 1);
});

test("a search holds the account's lock until it answers, so no read lands under it", async () => {
  const { server, tab } = await attached(3);
  server.holdSearches = true;
  const search = tab.search(ACCOUNT, KEY, 0, CALL);
  await waiting(() => server.waiting, 1);
  const window = tab.window(ACCOUNT, "inbox", 0);
  const first = await Promise.race([window.then(() => "window"), drained().then(() => "lock")]);
  expect(first).toBe("lock");
  server.releaseSearches();
  await expect(search).resolves.toMatchObject({ ok: true });
  await expect(window).resolves.toMatchObject({ ok: true });
});

test("a cancel gives the search up quietly and the next one runs", async () => {
  const logged = vi.spyOn(console, "error").mockImplementation(() => undefined);
  const { server, tab } = await attached(3);
  server.holdSearches = true;
  const search = tab.search(ACCOUNT, KEY, 0, CALL);
  await waiting(() => server.waiting, 1);
  await tab.cancelSearch(CALL.ticket);
  await expect(search).resolves.toMatchObject({ ok: false, failure: { method: null } });
  expect(server.waiting).toBe(0);
  expect(logged).not.toHaveBeenCalled();
  server.holdSearches = false;
  await expect(tab.search(ACCOUNT, KEY, 0, { ...CALL, ticket: "t2" })).resolves.toMatchObject({
    ok: true,
  });
  logged.mockRestore();
});

test("a search given up while it waits for the lock sends nothing", async () => {
  const { server, tab } = await attached(3);
  server.holdSearches = true;
  const first = tab.search(ACCOUNT, KEY, 0, CALL);
  await waiting(() => server.waiting, 1);
  const sent = server.posted().length;
  const second = tab.search(ACCOUNT, searchKeyOf({ q: "e3" }), 0, { ...CALL, ticket: "t2" });
  await tab.cancelSearch("t2");
  server.releaseSearches();
  await expect(first).resolves.toMatchObject({ ok: true });
  await expect(second).resolves.toMatchObject({ ok: false });
  expect(server.posted()).toHaveLength(sent);
});

test("a search the server refuses crosses with its method error", async () => {
  const logged = vi.spyOn(console, "error").mockImplementation(() => undefined);
  const { server, tab } = await attached(3);
  server.searchError = "serverUnavailable";
  await expect(tab.search(ACCOUNT, KEY, 0, CALL)).resolves.toEqual({
    ok: false,
    failure: {
      code: "unavailable",
      stopCause: null,
      limit: null,
      method: { type: "serverUnavailable", call: "q" },
    },
  });
  logged.mockRestore();
});

test("the senders answer from the held headers while a search holds the lock", async () => {
  const { server, tab } = await attached(3);
  await tab.window(ACCOUNT, "inbox", 0);
  server.holdSearches = true;
  const search = tab.search(ACCOUNT, KEY, 0, CALL);
  await waiting(() => server.waiting, 1);
  await expect(tab.senders(ACCOUNT, "san")).resolves.toEqual({
    ok: true,
    value: [{ email: "sanne@example.test", name: "Sanne", count: 3, newest: at(3) }],
  });
  await expect(tab.senders(ACCOUNT, "mo")).resolves.toEqual({ ok: true, value: [] });
  server.releaseSearches();
  await search;
});
