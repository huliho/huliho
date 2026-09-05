// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import {
  PASSWORD_MAX_CHARS,
  PASSWORD_MIN_CHARS,
  changePassword,
  fitsPasswordWindow,
} from "./password";

const CURRENT = "the old passphrase";
const NEXT = "a brand new passphrase";

function answer(
  status: number,
  body?: unknown,
  headers: Record<string, string> = {},
): Mock<typeof fetch> {
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockResolvedValue(
      new Response(body === undefined ? null : JSON.stringify(body), { status, headers }),
    );
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("the window counts code points, as the server does", () => {
  expect(fitsPasswordWindow("x".repeat(PASSWORD_MIN_CHARS))).toBe(true);
  expect(fitsPasswordWindow("x".repeat(PASSWORD_MIN_CHARS - 1))).toBe(false);
  expect(fitsPasswordWindow("x".repeat(PASSWORD_MAX_CHARS))).toBe(true);
  expect(fitsPasswordWindow("x".repeat(PASSWORD_MAX_CHARS + 1))).toBe(false);
  expect(fitsPasswordWindow("🔑".repeat(PASSWORD_MIN_CHARS))).toBe(true);
});

test("a change sends both passwords with the CSRF header", async () => {
  const fetchMock = answer(204);
  await changePassword({ current: CURRENT, new: NEXT });
  const [url, init] = fetchMock.mock.calls[0] ?? [];
  expect(url).toBe("/api/password");
  expect(init?.method).toBe("PUT");
  expect(new Headers(init?.headers).get("x-requested-with")).toBe("huliho");
  expect(init?.body).toBe(JSON.stringify({ current: CURRENT, new: NEXT }));
});

test("the forced step sends no current password at all", async () => {
  const fetchMock = answer(204);
  await changePassword({ new: NEXT });
  const [, init] = fetchMock.mock.calls[0] ?? [];
  expect(init?.body).toBe(JSON.stringify({ new: NEXT }));
});

test("a wrong current password and a vanished session are told apart", async () => {
  answer(401, { error: "invalid_credentials" });
  await expect(changePassword({ current: "wrong", new: NEXT })).rejects.toMatchObject({
    code: "invalid_credentials",
  });
  answer(401, { error: "unauthenticated" });
  await expect(changePassword({ current: CURRENT, new: NEXT })).rejects.toMatchObject({
    code: "unauthenticated",
  });
});

test("a rate limited answer carries its retry delay", async () => {
  answer(429, { error: "rate_limited" }, { "retry-after": "17" });
  await expect(changePassword({ current: CURRENT, new: NEXT })).rejects.toMatchObject({
    code: "rate_limited",
    retryAfterSeconds: 17,
  });
});

test("an unreachable server and any other refusal read as unavailable", async () => {
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("network down")));
  await expect(changePassword({ current: CURRENT, new: NEXT })).rejects.toMatchObject({
    code: "unavailable",
  });
  answer(500, { error: "internal" });
  await expect(changePassword({ current: CURRENT, new: NEXT })).rejects.toMatchObject({
    code: "unavailable",
  });
});
