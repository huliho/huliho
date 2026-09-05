// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import { LOGIN_MAX_BYTES, createUser, fetchUsers, fitsLogin, resetPassword } from "./users";

const ROW = {
  id: "u-1",
  name: "Mira",
  login: "mira@example.com",
  role: "owner",
  lastActiveAt: 1_778_750_400_000,
};
const NEW_USER = { name: "Jonas", login: "jonas@example.com", role: "member" } as const;
const ONE_TIME = "k7fq-2mzp-x4rt";
const ISSUED = { oneTimePassword: ONE_TIME, expiresAt: 1_778_836_800_000 };
// Where the Unicode white space set and the regex class \s part ways.
const NEXT_LINE = String.fromCodePoint(0x85);
const BYTE_ORDER_MARK = String.fromCodePoint(0xfeff);

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

test("a sign-in name is bounded in bytes and carries no whitespace", () => {
  expect(fitsLogin("jonas@example.com")).toBe(true);
  expect(fitsLogin("")).toBe(false);
  expect(fitsLogin("jonas example")).toBe(false);
  expect(fitsLogin("jonas\t")).toBe(false);
  expect(fitsLogin(`jonas${NEXT_LINE}`)).toBe(false);
  expect(fitsLogin(`jonas${BYTE_ORDER_MARK}`)).toBe(true);
  expect(fitsLogin("x".repeat(LOGIN_MAX_BYTES))).toBe(true);
  expect(fitsLogin("x".repeat(LOGIN_MAX_BYTES + 1))).toBe(false);
  expect(fitsLogin("é".repeat(LOGIN_MAX_BYTES))).toBe(false);
});

test("a user list parses row by row", async () => {
  answer(200, [ROW, { ...ROW, id: "u-2", role: "member", lastActiveAt: null }]);
  const rows = await fetchUsers();
  expect(rows.map((row) => row.id)).toEqual(["u-1", "u-2"]);
  expect(rows[1]?.lastActiveAt).toBeNull();
});

test("a malformed row and a failed list are errors", async () => {
  answer(200, [{ ...ROW, role: "boss" }]);
  await expect(fetchUsers()).rejects.toThrow(/invalid/i);
  answer(500, { error: "internal" });
  await expect(fetchUsers()).rejects.toThrow("users request failed");
});

test("a create sends the user with the CSRF header and parses the first password", async () => {
  const fetchMock = answer(201, { user: { ...ROW, ...NEW_USER, id: "u-3" }, ...ISSUED });
  const created = await createUser(NEW_USER);
  const [url, init] = fetchMock.mock.calls[0] ?? [];
  expect(url).toBe("/api/users");
  expect(init?.method).toBe("POST");
  expect(new Headers(init?.headers).get("x-requested-with")).toBe("huliho");
  expect(init?.body).toBe(JSON.stringify(NEW_USER));
  expect(created.user.id).toBe("u-3");
  expect(created.oneTimePassword).toBe(ISSUED.oneTimePassword);
});

test("a reset hits the encoded row and parses the password", async () => {
  const fetchMock = answer(200, ISSUED);
  const issued = await resetPassword("u 2");
  const [url, init] = fetchMock.mock.calls[0] ?? [];
  expect(url).toBe("/api/users/u%202/password-reset");
  expect(init?.method).toBe("POST");
  expect(new Headers(init?.headers).get("x-requested-with")).toBe("huliho");
  expect(issued.expiresAt).toBe(ISSUED.expiresAt);
});

test.each([
  [400, "invalid_request"],
  [401, "unauthenticated"],
  [403, "forbidden"],
  [404, "not_found"],
  [409, "login_taken"],
])("a %i refusal carries the code %s", async (status, code) => {
  answer(status, { error: code });
  await expect(createUser(NEW_USER)).rejects.toMatchObject({ code });
});

test("an unreachable server, an unnamed refusal and an odd body read as unavailable", async () => {
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("network down")));
  await expect(resetPassword("u-2")).rejects.toMatchObject({ code: "unavailable" });
  answer(500, { error: "internal" });
  await expect(resetPassword("u-2")).rejects.toMatchObject({ code: "unavailable" });
  answer(502, "<html>bad gateway</html>");
  await expect(resetPassword("u-2")).rejects.toMatchObject({ code: "unavailable" });
});
