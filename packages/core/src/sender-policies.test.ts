// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";
import type { Mock } from "vitest";

import { authenticationOf } from "./auth-results";
import type { Authentication, AuthenticationResults } from "./auth-results";
import {
  allowRemoteContent,
  blockRemoteContent,
  fetchSenderPolicies,
  grantFor,
  grantLoads,
  senderKey,
} from "./sender-policies";
import type { RemoteContentGrant } from "./sender-policies";

const SENDER = "news@shop.example";
const SERVER = "mx.example.net";
const ABSENT: Authentication = { status: "absent" };
const UNPARSEABLE: Authentication = { status: "unparseable" };

// The header Outlook.com and Exchange Online write: clauses and no server.
const MICROSOFT = authenticationOf([
  " spf=pass (sender IP is 203.0.113.7) smtp.mailfrom=shop.example;\r\n" +
    " dkim=pass (signature was verified) header.d=shop.example;dmarc=pass action=none\r\n" +
    " header.from=shop.example;compauth=pass reason=100",
]);

// The longest address the server keys a policy on.
const SENDER_MAX_BYTES = 320;

// A dotted capital I is two bytes and lowercases to three, so this many
// of them in front of "@x" come to the server's bound exactly.
const DOTTED_AT_BOUND = 106;

function pinned(authserv: string | null): RemoteContentGrant {
  return { allow: true, authserv };
}

function parsed(results: Partial<AuthenticationResults>): Authentication {
  return {
    status: "parsed",
    results: {
      server: SERVER,
      spf: "pass",
      dkim: "pass",
      dmarc: "pass",
      dmarcFrom: "shop.example",
      ...results,
    },
  };
}

function dotted(count: number): { email: string }[] {
  return [{ email: `${"İ".repeat(count)}@x` }];
}

function answer(status: number, body?: unknown): Mock<typeof fetch> {
  const fetched = vi
    .fn<typeof fetch>()
    .mockResolvedValue(new Response(body === undefined ? null : JSON.stringify(body), { status }));
  vi.stubGlobal("fetch", fetched);
  return fetched;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("a policy is keyed on the first address of From, lowercased", () => {
  expect(senderKey([{ email: "News@Shop.Example" }, { email: "other@shop.example" }])).toBe(SENDER);
});

test.each([
  ["no From at all", null],
  ["an empty From", []],
  ["an address without a domain", [{ email: "news" }]],
  ["an address that ends in the at sign", [{ email: "news@" }]],
  ["an address that starts with the at sign", [{ email: "@shop.example" }]],
  ["a control character", [{ email: "news@shop.example\r\nBcc: x@evil.example" }]],
  ["an address past the server's bound", [{ email: `${"a".repeat(SENDER_MAX_BYTES)}@x.example` }]],
])("a message with %s has no key", (_label, from) => {
  expect(senderKey(from)).toBeNull();
});

test("the bound on a key counts bytes after lowercasing, as the server does", () => {
  expect(senderKey(dotted(DOTTED_AT_BOUND))).not.toBeNull();
  expect(senderKey(dotted(DOTTED_AT_BOUND + 1))).toBeNull();
});

test("a grant loads a message the same server passed for the sender's domain", () => {
  expect(grantLoads(pinned(SERVER), SENDER, parsed({}))).toBe(true);
  // SPF and DKIM do not decide; the DMARC verdict for the From domain does.
  expect(grantLoads(pinned(SERVER), SENDER, parsed({ spf: "fail", dkim: "none" }))).toBe(true);
});

test.each([
  ["another server stamped it", parsed({ server: "mx.other.example" })],
  ["the header names no server", parsed({ server: null })],
  ["DMARC failed", parsed({ dmarc: "fail" })],
  ["DMARC was not checked", parsed({ dmarc: "none" })],
  ["DMARC came out unknown", parsed({ dmarc: "unknown" })],
  ["DMARC passed for another domain", parsed({ dmarcFrom: "bank.example" })],
  ["DMARC passed for a parent domain", parsed({ dmarcFrom: "example" })],
  ["DMARC names no domain", parsed({ dmarcFrom: null })],
  ["DMARC names something that is no domain", parsed({ dmarcFrom: "shop.example/x" })],
  ["the header cannot be read", UNPARSEABLE],
  ["no header came where one was pinned", ABSENT],
])("a grant pinned on a server stays blocked when %s", (_label, authentication) => {
  expect(grantLoads(pinned(SERVER), SENDER, authentication)).toBe(false);
});

test("a grant given without a header loads a message without one and blocks every header", () => {
  expect(grantLoads(pinned(null), SENDER, ABSENT)).toBe(true);
  expect(grantLoads(pinned(null), SENDER, parsed({}))).toBe(false);
  expect(grantLoads(pinned(null), SENDER, MICROSOFT)).toBe(false);
  expect(grantLoads(pinned(null), SENDER, UNPARSEABLE)).toBe(false);
});

test("a grant given on a header that names no server loads that shape and nothing else", () => {
  expect(grantLoads(pinned(""), SENDER, MICROSOFT)).toBe(true);
  expect(grantLoads(pinned(""), SENDER, parsed({ server: null, dmarc: "fail" }))).toBe(false);
  // The server stamped a verdict at the grant and stamps none now.
  expect(grantLoads(pinned(""), SENDER, ABSENT)).toBe(false);
  expect(grantLoads(pinned(""), SENDER, parsed({}))).toBe(false);
});

test("the domain compares in its ASCII form, whichever spelling the header and From use", () => {
  const unicode = "news@bücher.example";
  const ascii = "news@xn--bcher-kva.example";
  expect(grantLoads(pinned(SERVER), unicode, parsed({ dmarcFrom: "xn--bcher-kva.example" }))).toBe(
    true,
  );
  expect(grantLoads(pinned(SERVER), ascii, parsed({ dmarcFrom: "bücher.example" }))).toBe(true);
  expect(grantLoads(pinned(SERVER), unicode, parsed({ dmarcFrom: "bucher.example" }))).toBe(false);
  // A From that closes its domain with a dot names the same domain.
  expect(grantLoads(pinned(SERVER), "news@shop.example.", parsed({}))).toBe(true);
});

test("the grant a reader can give pins what the topmost header says", () => {
  expect(grantFor(SENDER, parsed({}))).toEqual(pinned(SERVER));
  expect(grantFor(SENDER, MICROSOFT)).toEqual(pinned(""));
  expect(grantFor(SENDER, ABSENT)).toEqual(pinned(null));
});

test("no grant can be given on a message that would not load under it", () => {
  expect(grantFor(SENDER, parsed({ dmarc: "fail" }))).toBeNull();
  expect(grantFor(SENDER, parsed({ dmarcFrom: "bank.example" }))).toBeNull();
  expect(grantFor(SENDER, UNPARSEABLE)).toBeNull();
});

test("a forged header buys nothing where the server stamps its own and nothing more than the address where it stamps none", () => {
  const forged = " mx.example.net; dmarc=pass header.from=shop.example";
  const stamped = " mx.example.net; dmarc=fail header.from=shop.example";
  // The server's header sits on top, so the forged one below it is never read.
  expect(grantLoads(pinned(SERVER), SENDER, authenticationOf([stamped, forged]))).toBe(false);
  // A grant given on mail without a header does not take a forged one.
  expect(grantLoads(pinned(null), SENDER, authenticationOf([forged]))).toBe(false);
  expect(grantLoads(pinned(null), SENDER, authenticationOf([]))).toBe(true);
});

test("the list parses and a row off the shape is refused at the boundary", async () => {
  const rows = [
    { sender: SENDER, key: "remoteContent", value: { allow: true, authserv: SERVER } },
    { sender: "a@b.example", key: "remoteContent", value: { allow: true, authserv: null } },
  ];
  const fetched = answer(200, rows);
  expect(await fetchSenderPolicies()).toEqual(rows);
  expect(fetched).toHaveBeenCalledWith("/api/sender-policies");
  answer(200, [{ sender: SENDER, key: "remoteContent", value: { allow: false, authserv: null } }]);
  await expect(fetchSenderPolicies()).rejects.toThrow(/invalid/i);
  answer(200, [{ sender: SENDER, key: "other", value: { allow: true, authserv: null } }]);
  await expect(fetchSenderPolicies()).rejects.toThrow(/invalid/i);
  answer(500, { error: "internal" });
  await expect(fetchSenderPolicies()).rejects.toThrow("status 500");
});

test("a grant is one PUT under the encoded sender with the pin and the header every write carries", async () => {
  const fetched = answer(204);
  await allowRemoteContent("a/b+c@shop.example", pinned(SERVER));
  expect(fetched).toHaveBeenCalledWith("/api/sender-policies/a%2Fb%2Bc%40shop.example", {
    method: "PUT",
    headers: { "content-type": "application/json", "x-requested-with": "huliho" },
    body: JSON.stringify({ key: "remoteContent", value: { allow: true, authserv: SERVER } }),
  });
});

test("taking a grant back is one DELETE under the sender and the policy", async () => {
  const fetched = answer(204);
  await blockRemoteContent(SENDER);
  expect(fetched).toHaveBeenCalledWith("/api/sender-policies/news%40shop.example/remoteContent", {
    method: "DELETE",
    headers: { "x-requested-with": "huliho" },
  });
});

test("a write the server refuses reads as unauthenticated or unavailable", async () => {
  answer(401, { error: "unauthenticated" });
  await expect(allowRemoteContent(SENDER, pinned(null))).rejects.toMatchObject({
    name: "SenderPoliciesError",
    code: "unauthenticated",
  });
  answer(400, { error: "invalid_request" });
  await expect(allowRemoteContent(SENDER, pinned(null))).rejects.toMatchObject({
    code: "unavailable",
  });
  vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockRejectedValue(new TypeError("down")));
  await expect(blockRemoteContent(SENDER)).rejects.toMatchObject({ code: "unavailable" });
});
