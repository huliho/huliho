// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test } from "vitest";

import { FRAGMENT_BYTES_MAX, openHref, readFragment } from "./fragment";
import { forgetLinkKey, hasLinkKey, linkKey } from "./link-key";
import { planLink, targetPieces } from "./plan";

const OWN_HOST = "mail.example.test";
const INVALID = { kind: "invalid" };

// The fragment of a link this device made.
function own(target: string, text = ""): string {
  return (openHref({ target, text, key: linkKey() }) ?? "").slice("/open".length);
}

// The fragment of a link made elsewhere: without a key, or with another.
function foreign(target: string, key = ""): string {
  return (openHref({ target, text: "", key }) ?? "").slice("/open".length);
}

afterEach(() => {
  localStorage.clear();
});

test("the device's key is made once, kept and told apart from any other", () => {
  expect(hasLinkKey("")).toBe(false);
  const key = linkKey();
  expect(key).toMatch(/^[0-9a-f-]{36}$/);
  expect(linkKey()).toBe(key);
  expect(hasLinkKey(key)).toBe(true);
  expect(hasLinkKey(`${key}x`)).toBe(false);
  expect(hasLinkKey("")).toBe(false);
  forgetLinkKey();
  expect(hasLinkKey(key)).toBe(false);
  expect(linkKey()).not.toBe(key);
});

test("a fragment carries the target, the text and the key and nothing reaches a query", () => {
  const href = openHref({
    target: "https://shop.example/sale?x=1&y=2#top",
    text: "  See the sale & more  ",
    key: "k1",
  });
  expect(href?.startsWith("/open#")).toBe(true);
  expect(href).not.toContain("?");
  expect(readFragment(href?.slice("/open".length) ?? "")).toEqual({
    target: "https://shop.example/sale?x=1&y=2#top",
    text: "See the sale & more",
    key: "k1",
  });
});

test("a fragment keeps to its bound: the text is cut, then left out, then the link has no address", () => {
  const cut = openHref({ target: "https://shop.example/", text: "a".repeat(1000), key: "k" });
  expect(readFragment(cut?.slice("/open".length) ?? "")?.text).toHaveLength(256);
  const long = `https://shop.example/${"a".repeat(3900)}`;
  const bare = openHref({ target: long, text: "b".repeat(200), key: "k" });
  expect(readFragment(bare?.slice("/open".length) ?? "")).toEqual({
    target: long,
    text: null,
    key: "k",
  });
  expect(bare?.length).toBeLessThanOrEqual(FRAGMENT_BYTES_MAX + "/open#".length);
  expect(
    openHref({ target: `https://shop.example/${"a".repeat(4100)}`, text: "", key: "k" }),
  ).toBeNull();
});

test.each(["", "#", "#t=only+text", "#u=", `#u=${"a".repeat(FRAGMENT_BYTES_MAX)}`])(
  "a fragment without a target or past the bound carries no link: %s",
  (hash) => {
    expect(readFragment(hash)).toBeNull();
  },
);

test("a fragment of any bytes is read without a throw", () => {
  expect(readFragment("#u=%E0%A4%A&t=%ZZ&k=%")).toEqual({ target: "�%A", text: "%ZZ", key: "%" });
});

test("a plain link this device made leaves for its target at once", () => {
  expect(planLink(own("https://shop.example/sale?x=1#top", "See the sale"), OWN_HOST)).toEqual({
    kind: "leave",
    url: "https://shop.example/sale?x=1#top",
    host: "shop.example",
  });
});

test("a link made elsewhere asks first, whatever it opens", () => {
  const unverified = { kind: "unverified", host: "evil.example" };
  expect(planLink(foreign("https://evil.example/"), OWN_HOST)).toEqual({
    kind: "ask",
    url: "https://evil.example/",
    reason: unverified,
  });
  expect(planLink(foreign("https://evil.example/", "a-guess"), OWN_HOST)).toMatchObject({
    reason: unverified,
  });
  linkKey();
  expect(planLink(foreign("https://evil.example/", "a-guess"), OWN_HOST)).toMatchObject({
    reason: unverified,
  });
});

test.each([
  ["https://mybank-secure.example.net/login", "mybank.example", "mismatch"],
  ["https://xn--mybnk-fsa.example/inloggen", "Sign in", "international"],
  ["https://mail.example.test/settings/accounts", "Settings", "own"],
])("a link with a risk asks even with the device's key: %s", (target, text, kind) => {
  expect(planLink(own(target, text), OWN_HOST)).toMatchObject({
    kind: "ask",
    url: target,
    reason: { kind },
  });
});

// A target padded with a run of characters, so the fragment has less
// and less room for the link's text.
function padded(run: number): string {
  return `https://evil.example/login?p=${"a".repeat(run)}`;
}

// The fragment of a padded link whose text names another host.
function hiding(run: number): string {
  return own(padded(run), "mybank.example");
}

// Runs around the one at which the text finds no room, and the first
// and the last of them at which it is left out.
const RUNS = { from: 3900, to: 4100, firstWithout: 3998, lastWithout: 4014 };

test("a link whose text found no room asks first, however its target was padded", () => {
  for (let run = RUNS.from; run <= RUNS.to; run += 1) {
    expect(planLink(hiding(run), OWN_HOST).kind).not.toBe("leave");
  }
  expect(planLink(hiding(RUNS.firstWithout - 1), OWN_HOST)).toMatchObject({
    kind: "ask",
    reason: { kind: "mismatch" },
  });
  for (const run of [RUNS.firstWithout, RUNS.lastWithout]) {
    expect(readFragment(hiding(run))?.text).toBeNull();
    expect(planLink(hiding(run), OWN_HOST)).toMatchObject({
      kind: "ask",
      reason: { kind: "unverified" },
    });
  }
  expect(planLink(hiding(RUNS.lastWithout + 1), OWN_HOST)).toEqual(INVALID);
});

test("a text that leads with characters a reader never sees still carries the host it names", () => {
  const unseen = String.fromCodePoint(0x200b).repeat(300);
  const fragment = own("https://evil.example/", `${unseen}mybank.example`);
  expect(readFragment(fragment)?.text).toBe("mybank.example");
  expect(planLink(fragment, OWN_HOST)).toMatchObject({ kind: "ask", reason: { kind: "mismatch" } });
});

test("a mail address goes to the mail program with the device's key and nowhere without", () => {
  expect(planLink(own("mailto:sanne@example.test?subject=hi"), OWN_HOST)).toEqual({
    kind: "mail",
    url: "mailto:sanne@example.test?subject=hi",
  });
  expect(planLink(foreign("mailto:sanne@example.test"), OWN_HOST)).toEqual(INVALID);
});

test.each([
  "javascript:top.__x=1",
  " JavaScript:top.__x=1",
  "data:text/html,<script>top.__x=1</script>",
  "blob:https://mail.example.test/1",
  "file:///etc/passwd",
  "ftp://shop.example/x",
  "/settings",
  "//evil.example/",
  "shop.example",
  "https://",
  "https://shop.example/api/x",
  "https://mail.example.test/api/session",
])(
  "a target the route never follows opens nothing, with the device's key or without: %s",
  (target) => {
    expect(planLink(own(target), OWN_HOST)).toEqual(INVALID);
    expect(planLink(foreign(target), OWN_HOST)).toEqual(INVALID);
  },
);

test("a fragment that carries no link opens nothing", () => {
  expect(planLink("", OWN_HOST)).toEqual(INVALID);
  expect(planLink("#k=x&t=y", OWN_HOST)).toEqual(INVALID);
});

test("the target shows in three pieces, the one the reason is about in the middle", () => {
  const url = "https://mybank-secure.example.net:8443/login/verify?session=8f3a#top";
  const mismatch = {
    kind: "mismatch",
    text: "mybank.example",
    host: "mybank-secure.example.net",
  } as const;
  expect(targetPieces(url, mismatch)).toEqual([
    "https://",
    "mybank-secure.example.net:8443",
    "/login/verify?session=8f3a#top",
  ]);
  expect(targetPieces(url, { kind: "unverified", host: "mybank-secure.example.net" })[1]).toBe(
    "mybank-secure.example.net:8443",
  );
  const inside = "https://mail.example.test/settings/accounts?tab=1";
  expect(targetPieces(inside, { kind: "own", path: "/settings/accounts" })).toEqual([
    "https://mail.example.test",
    "/settings/accounts",
    "?tab=1",
  ]);
});
