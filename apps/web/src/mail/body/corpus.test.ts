// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { readFileSync } from "node:fs";
import { join } from "node:path";

import { expect, test } from "vitest";

import { BLANK_PIXEL } from "./css";
import { DOWNLOAD_PREFIX, PROXY_PREFIX, built } from "./frame-rig";
import { GENERIC_ATTRIBUTES, LINK_REL, LINK_TARGET, TAGS, URL_ATTRIBUTES } from "./profile";

// The corpus the server's layer runs: the DOMPurify fixtures and the
// mail cases. Here each payload meets the window's pipeline as the
// sender wrote it, as if the server's layer had let it through whole.
const DOMPURIFY = readFileSync(
  join(import.meta.dirname, "../../../../../crates/server/tests/xss/dompurify-expect.json"),
  "utf8",
);
const MAIL = readFileSync(
  join(import.meta.dirname, "../../../../../crates/server/tests/xss/mail.json"),
  "utf8",
);

// The DOMPurify fixtures hold at least this many cases.
const DOMPURIFY_FLOOR = 200;

// Long enough for jsdom to build every fixture twice on a shared runner.
const CORPUS_TIMEOUT_MS = 30_000;

interface Case {
  title: string;
  payload: string;
}

function isCase(value: unknown): value is { title?: unknown; payload: string } {
  return (
    typeof value === "object" &&
    value !== null &&
    "payload" in value &&
    typeof value.payload === "string"
  );
}

function cases(source: string): Case[] {
  const read: unknown = JSON.parse(source);
  if (!Array.isArray(read)) {
    throw new TypeError("the corpus holds no list");
  }
  return read.filter(isCase).map((entry) => ({
    title: typeof entry.title === "string" ? entry.title : entry.payload,
    payload: entry.payload,
  }));
}

const ELEMENTS = new Set<string>(TAGS);
const ATTRIBUTES = new Set<string>([...GENERIC_ATTRIBUTES, "target", "rel"]);

// One url() of a serialized value and what it names.
const URL_TOKEN = /url\(\s*["']?([^"')]*)/gi;

function isOurs(url: string, remote: boolean): boolean {
  const loads = url.startsWith(DOWNLOAD_PREFIX) || (remote && url.startsWith(PROXY_PREFIX));
  return loads || url === BLANK_PIXEL || url.toLowerCase().startsWith("data:image/");
}

function attributeProblems(element: Element, remote: boolean): string[] {
  return element.getAttributeNames().flatMap((name) => {
    const value = element.getAttribute(name) ?? "";
    const carried = URL_ATTRIBUTES.some(
      ([tag, attr]) => tag === element.localName && attr === name,
    );
    if (name === "href") {
      return carried && value.startsWith("/open#") ? [] : [`href ${value}`];
    }
    if (carried) {
      return isOurs(value, remote) ? [] : [`${name} ${value}`];
    }
    return ATTRIBUTES.has(name) ? [] : [`attribute ${name} on ${element.localName}`];
  });
}

function linkProblems(element: Element): string[] {
  if (element.localName !== "a" || !element.hasAttribute("href")) {
    return [];
  }
  const aimed =
    element.getAttribute("target") === LINK_TARGET && element.getAttribute("rel") === LINK_REL;
  return aimed ? [] : ["a link without its target and rel"];
}

function cssProblems(text: string, remote: boolean): string[] {
  const urls = Array.from(text.matchAll(URL_TOKEN), ([, url]) => url ?? "");
  const foreign = urls.filter((url) => !isOurs(url, remote)).map((url) => `url ${url}`);
  const words = ["@import", "@font-face", "expression(", "\\"].filter((word) =>
    text.toLowerCase().includes(word),
  );
  return [...foreign, ...words.map((word) => `css ${word}`)];
}

// Everything in a built document that the frame must never get: an
// element or attribute off the allowlist, a link around the open
// route, a URL the policy did not write and CSS that could load.
function problems(html: string, remote: boolean): string[] {
  const page = new DOMParser().parseFromString(html, "text/html");
  const elements = Array.from(page.body.querySelectorAll("*"));
  return [
    ...elements
      .filter((element) => !ELEMENTS.has(element.localName))
      .map((element) => element.localName),
    ...elements.flatMap((element) => attributeProblems(element, remote)),
    ...elements.flatMap((element) => linkProblems(element)),
    ...elements.flatMap((element) => cssProblems(element.getAttribute("style") ?? "", remote)),
    ...Array.from(page.body.querySelectorAll("style")).flatMap((element) =>
      cssProblems(element.textContent, remote),
    ),
    ...(page.head.querySelectorAll("script, link, base, title").length > 0 ? ["head content"] : []),
    ...(page.head.querySelectorAll("style").length === 1 ? [] : ["a style block in the head"]),
  ];
}

function failures(source: string, remote: boolean, adapt = false): string[] {
  const options = adapt ? { remote, theme: "dark" as const, adapt } : { remote };
  return cases(source).flatMap(({ title, payload }) =>
    problems(built(payload, options).html, remote).map((problem) => `${title}: ${problem}`),
  );
}

test(
  "every DOMPurify fixture comes out within the allowlist, blocked and allowed",
  { timeout: CORPUS_TIMEOUT_MS },
  () => {
    expect(cases(DOMPURIFY).length).toBeGreaterThanOrEqual(DOMPURIFY_FLOOR);
    expect(failures(DOMPURIFY, false)).toEqual([]);
    expect(failures(DOMPURIFY, true)).toEqual([]);
  },
);

test("every mail case comes out within the allowlist, blocked and allowed", () => {
  expect(failures(MAIL, false)).toEqual([]);
  expect(failures(MAIL, true)).toEqual([]);
});

test(
  "every case comes out within the allowlist under the dark adaptation as well",
  { timeout: CORPUS_TIMEOUT_MS },
  () => {
    expect(failures(DOMPURIFY, false, true)).toEqual([]);
    expect(failures(MAIL, false, true)).toEqual([]);
    const hidden =
      '<table><tr><td bgcolor="var(--a) url(https://probe.invalid/leak.png)">x</td></tr></table>';
    const { html, page, adapted } = built(hidden, { theme: "dark", adapt: true });
    expect(adapted).toBe(true);
    // The attribute stays what the sender wrote; no style is made of it.
    expect(page.body.querySelector("td")?.hasAttribute("style")).toBe(false);
    expect(html).not.toContain("oklch(from var(--a)");
    expect(problems(html, false)).toEqual([]);
  },
);

// What a built document holds that could run, or load from another
// host without being asked.
function live(html: string, page: Document): string[] {
  const handlers = Array.from(page.querySelectorAll("*")).flatMap((element) =>
    element.getAttributeNames().filter((name) => name.startsWith("on")),
  );
  const loads = Array.from(
    page.body.querySelectorAll("[src], [background]"),
    (element) => element.getAttribute("src") ?? element.getAttribute("background") ?? "",
  ).filter((url) => !url.startsWith("data:") && !url.startsWith("/api/"));
  return [
    ...Array.from(page.querySelectorAll("script"), () => "a script element"),
    ...handlers,
    ...loads,
    ...(html.includes("javascript:") ? ["a javascript address"] : []),
  ];
}

test("no canary of a mail case survives as code and no case names a remote address unasked", () => {
  const found = cases(MAIL).flatMap(({ title, payload }) => {
    const { html, page } = built(payload);
    return live(html, page).map((problem) => `${title}: ${problem}`);
  });
  expect(found).toEqual([]);
});

test("a built document fed back in comes out clean and no larger", () => {
  const found = cases(MAIL).flatMap(({ title, payload }) => {
    const once = built(payload).page.body.innerHTML;
    const again = built(once);
    const grown = again.page.body.innerHTML.length > once.length ? ["grew on its way back"] : [];
    return [...problems(again.html, false), ...grown].map((problem) => `${title}: ${problem}`);
  });
  expect(found).toEqual([]);
});
