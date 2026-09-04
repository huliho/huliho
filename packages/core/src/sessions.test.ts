// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import { fetchSessions, revokeOtherSessions, revokeSession } from "./sessions";

const ROW = {
  id: "s-1",
  current: true,
  device: { browser: "Firefox", os: "Linux", phone: false, installed: false },
  address: "203.0.113.7",
  createdAt: 1_778_750_000_000,
  lastSeenAt: 1_778_750_400_000,
};

function answer(status: number, body?: unknown): Mock<typeof fetch> {
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockResolvedValue(new Response(body === undefined ? null : JSON.stringify(body), { status }));
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("a session list parses row by row", async () => {
  answer(200, [ROW, { ...ROW, id: "s-2", current: false, address: null }]);
  const rows = await fetchSessions();
  expect(rows.map((row) => row.id)).toEqual(["s-1", "s-2"]);
  expect(rows[1]?.address).toBeNull();
});

test("a malformed row is rejected at the boundary", async () => {
  answer(200, [{ ...ROW, device: { browser: 7 } }]);
  await expect(fetchSessions()).rejects.toThrow(/invalid input/i);
});

test("a failed list is an error", async () => {
  answer(500, { error: "internal" });
  await expect(fetchSessions()).rejects.toThrow("sessions request failed");
});

test("a revoke sends the CSRF header and can outlive the page", async () => {
  const fetchMock = answer(204);
  await revokeSession("s 2", { keepalive: true });
  const [url, init] = fetchMock.mock.calls[0] ?? [];
  expect(url).toBe("/api/sessions/s%202");
  expect(init?.method).toBe("DELETE");
  expect(init?.keepalive).toBe(true);
  expect(new Headers(init?.headers).get("x-requested-with")).toBe("huliho");
});

test("revoking the others hits the collection without keepalive by default", async () => {
  const fetchMock = answer(204);
  await revokeOtherSessions();
  const [url, init] = fetchMock.mock.calls[0] ?? [];
  expect(url).toBe("/api/sessions");
  expect(init?.keepalive).toBe(false);
});

test("a row that is already gone counts as revoked", async () => {
  answer(404, { error: "not_found" });
  await expect(revokeSession("s-9")).resolves.toBeUndefined();
});

test("any other refusal is an error", async () => {
  answer(400, { error: "invalid_request" });
  await expect(revokeSession("s-1")).rejects.toThrow("revoke request failed");
});
