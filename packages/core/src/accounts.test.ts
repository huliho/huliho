// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import {
  addAccount,
  discoverServer,
  fetchAccounts,
  fetchConsent,
  replaceCredential,
  startConsent,
} from "./accounts";
import type { NewAccountInput } from "./accounts";

const ROW = {
  id: "acc-1",
  address: "sanne@fastmail.com",
  name: "Fastmail",
  provider: "fastmail",
  kind: "jmap",
  authMethod: "bearer",
  stoppedCause: null,
  stoppedAt: null,
  createdAt: 1_778_750_400_000,
};
const LIST = { accounts: [ROW], probeIntervalMinutes: 15 };
const FOUND = {
  status: "found",
  provider: "fastmail",
  kind: "jmap",
  target: { kind: "jmap", sessionUrl: "https://api.fastmail.com/jmap/session" },
  credentialKind: "apiToken",
  host: "api.fastmail.com",
  oauthAvailable: false,
};
const INPUT: NewAccountInput = {
  address: "sanne@fastmail.com",
  provider: "fastmail",
  target: { kind: "jmap", sessionUrl: "https://api.fastmail.com/jmap/session" },
  credential: { kind: "bearer", token: "fmu1-example" },
};
const RETRY_SECONDS = 90;

function answer(status: number, body?: unknown, headers: HeadersInit = {}): Mock<typeof fetch> {
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

test("the list parses with its probe interval", async () => {
  answer(200, LIST);
  const list = await fetchAccounts();
  expect(list.accounts.map((row) => row.id)).toEqual(["acc-1"]);
  expect(list.probeIntervalMinutes).toBe(15);
});

test("a malformed row and a failed list are errors", async () => {
  answer(200, { ...LIST, accounts: [{ ...ROW, provider: "hotmail" }] });
  await expect(fetchAccounts()).rejects.toThrow(/invalid/i);
  answer(500, { error: "internal" });
  await expect(fetchAccounts()).rejects.toThrow("accounts request failed");
});

test("discovery sends the address with the CSRF header and answers the server or null", async () => {
  const fetchMock = answer(200, FOUND);
  const found = await discoverServer("sanne@fastmail.com");
  const [url, init] = fetchMock.mock.calls[0] ?? [];
  expect(url).toBe("/api/accounts/discover");
  expect(init?.method).toBe("POST");
  expect(new Headers(init?.headers).get("x-requested-with")).toBe("huliho");
  expect(init?.body).toBe(JSON.stringify({ address: "sanne@fastmail.com" }));
  expect(found?.host).toBe("api.fastmail.com");
  expect(found?.credentialKind).toBe("apiToken");
  answer(200, { status: "notFound" });
  expect(await discoverServer("sanne@dekker-mail.nl")).toBeNull();
});

test("a found answer without its target is an error", async () => {
  answer(200, { ...FOUND, target: { kind: "imap" } });
  await expect(discoverServer("sanne@fastmail.com")).rejects.toThrow(/invalid/i);
});

test("a connect sends the target with the credential once and parses the row", async () => {
  const fetchMock = answer(201, ROW);
  const row = await addAccount(INPUT);
  const [url, init] = fetchMock.mock.calls[0] ?? [];
  expect(url).toBe("/api/accounts");
  expect(init?.method).toBe("POST");
  expect(init?.body).toBe(JSON.stringify(INPUT));
  expect(row.name).toBe("Fastmail");
});

test("a reconnect puts the credential on the encoded row", async () => {
  const fetchMock = answer(200, ROW);
  await replaceCredential("acc 1", { kind: "password", password: "wrong horse" });
  const [url, init] = fetchMock.mock.calls[0] ?? [];
  expect(url).toBe("/api/accounts/acc%201/credentials");
  expect(init?.method).toBe("PUT");
  expect(init?.body).toBe(
    JSON.stringify({ credential: { kind: "password", password: "wrong horse" } }),
  );
});

test("a consent starts with the preset and the address and answers the window's URL", async () => {
  const started = { url: "https://accounts.google.com/o/oauth2/v2/auth?state=s1", state: "s1" };
  const fetchMock = answer(200, started);
  expect(await startConsent({ provider: "gmail", address: "sanne@gmail.com" })).toEqual(started);
  const [url, init] = fetchMock.mock.calls[0] ?? [];
  expect(url).toBe("/api/accounts/oauth/start");
  expect(init?.method).toBe("POST");
  expect(new Headers(init?.headers).get("x-requested-with")).toBe("huliho");
  expect(init?.body).toBe(JSON.stringify({ provider: "gmail", address: "sanne@gmail.com" }));
});

test("a reconnect consent names the row; a start the instance cannot serve names the cause", async () => {
  const fetchMock = answer(200, { url: "https://x.test/auth", state: "s2" });
  await startConsent({ provider: "gmail", address: "sanne@gmail.com", accountId: "acc-1" });
  expect(fetchMock.mock.calls[0]?.[1]?.body).toBe(
    JSON.stringify({ provider: "gmail", address: "sanne@gmail.com", accountId: "acc-1" }),
  );
  answer(409, { error: "provider_not_configured" });
  await expect(
    startConsent({ provider: "gmail", address: "sanne@gmail.com" }),
  ).rejects.toMatchObject({
    code: "provider_not_configured",
  });
});

test("a start whose URL is not https is rejected at the boundary", async () => {
  const input = { provider: "gmail", address: "sanne@gmail.com" } as const;
  answer(200, { url: "javascript:alert(1)", state: "s1" });
  await expect(startConsent(input)).rejects.toThrow(/invalid/i);
  answer(200, { url: "http://accounts.google.com/o/oauth2/v2/auth", state: "s1" });
  await expect(startConsent(input)).rejects.toThrow(/invalid/i);
});

test("the poll parses the three outcomes; a 404 reads as gone and a 401 as the session ending", async () => {
  answer(200, { status: "pending" });
  expect(await fetchConsent("s1")).toEqual({ status: "pending" });
  answer(200, { status: "done", accountId: "acc-2" });
  expect(await fetchConsent("s1")).toEqual({ status: "done", accountId: "acc-2" });
  answer(200, { status: "denied", cause: "accessDenied" });
  expect(await fetchConsent("s1")).toEqual({ status: "denied", cause: "accessDenied" });
  answer(404, { error: "not_found" });
  expect(await fetchConsent("s1")).toEqual({ status: "gone" });
  answer(401, { error: "unauthenticated" });
  await expect(fetchConsent("s1")).rejects.toMatchObject({ code: "unauthenticated" });
  answer(200, { status: "denied", cause: "somethingElse" });
  await expect(fetchConsent("s1")).rejects.toThrow(/invalid/i);
});

test("the poll names the consent in its path; an unreachable server reads as unavailable", async () => {
  const fetchMock = answer(200, { status: "pending" });
  await fetchConsent("s 1/x");
  expect(fetchMock.mock.calls[0]?.[0]).toBe("/api/accounts/oauth/pending/s%201%2Fx");
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("down")));
  await expect(fetchConsent("s1")).rejects.toMatchObject({ code: "unavailable" });
});

test.each([
  [400, "invalid_request"],
  [401, "upstream_credentials"],
  [401, "unauthenticated"],
  [502, "upstream_unreachable"],
  [400, "upstream_insecure"],
  [400, "upstream_unsupported"],
  [400, "smtp_auth_unavailable"],
  [404, "not_found"],
])("a %i refusal carries the code %s", async (status, code) => {
  answer(status, { error: code });
  await expect(addAccount(INPUT)).rejects.toMatchObject({ code });
});

test("a rate-limited answer carries how long to wait", async () => {
  answer(429, { error: "rate_limited" }, { "retry-after": String(RETRY_SECONDS) });
  await expect(discoverServer("sanne@fastmail.com")).rejects.toMatchObject({
    code: "rate_limited",
    retryAfterSeconds: RETRY_SECONDS,
  });
});

test("an unreachable server, an unnamed refusal and an odd body read as unavailable", async () => {
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("network down")));
  await expect(addAccount(INPUT)).rejects.toMatchObject({ code: "unavailable" });
  answer(500, { error: "internal" });
  await expect(addAccount(INPUT)).rejects.toMatchObject({ code: "unavailable" });
  answer(502, "<html>bad gateway</html>");
  await expect(addAccount(INPUT)).rejects.toMatchObject({ code: "unavailable" });
});
