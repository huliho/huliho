// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import { SignInError, fetchSession, signIn, signOut } from "./session";

const SESSION_BODY = {
  user: { id: "u1", login: "mira@example.com", role: "owner" },
  organization: { id: "o1", name: "mira@example.com" },
};

function answer(status: number, body?: unknown, headers: Record<string, string> = {}): void {
  vi.stubGlobal(
    "fetch",
    vi
      .fn()
      .mockResolvedValue(
        new Response(body === undefined ? null : JSON.stringify(body), { status, headers }),
      ),
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("a 401 session answer means signed out, not an error", async () => {
  answer(401, { error: "unauthenticated" });
  expect(await fetchSession()).toBeNull();
});

test("a valid session answer parses into user and organization", async () => {
  answer(200, SESSION_BODY);
  const session = await fetchSession();
  expect(session?.user.login).toBe("mira@example.com");
  expect(session?.organization.id).toBe("o1");
});

test("a malformed session answer is rejected at the boundary", async () => {
  answer(200, { user: { id: "u1" } });
  await expect(fetchSession()).rejects.toThrow(/invalid input/i);
});

test("wrong credentials surface as a typed failure", async () => {
  answer(401, { error: "invalid_credentials" });
  await expect(signIn("mira@example.com", "wrong")).rejects.toMatchObject({
    code: "invalid_credentials",
  });
});

test("a rate limited answer carries its retry delay", async () => {
  answer(429, { error: "rate_limited" }, { "retry-after": "17" });
  await expect(signIn("mira@example.com", "wrong")).rejects.toMatchObject({
    code: "rate_limited",
    retryAfterSeconds: 17,
  });
});

test("an unreachable server surfaces as unavailable", async () => {
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("network down")));
  await expect(signIn("mira@example.com", "pw")).rejects.toMatchObject({
    code: "unavailable",
  });
});

test("a successful sign-in resolves without a value", async () => {
  answer(204);
  await expect(signIn("mira@example.com", "right")).resolves.toBeUndefined();
});

test("a failed sign-out is an error, not a silent pass", async () => {
  answer(500, { error: "internal" });
  await expect(signOut()).rejects.toThrow("sign-out request failed");
  expect(new SignInError("unavailable").retryAfterSeconds).toBe(0);
});
