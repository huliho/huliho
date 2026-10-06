// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { BODY_VALUE_BYTES, BODY_VALUE_BYTES_LARGE } from "@huliho/core";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";

import {
  PLAIN_LINES_MAX,
  PLAIN_LINKS_MAX,
  PLAIN_RUNS_MAX,
  PLAIN_SEARCH_MAX,
  PlainText,
  firstLines,
} from "./plain-text";

const LINKS = { ownHost: "mail.example.test", linkKey: "key-1" };

function rendered(text: string, flowed: { delSp: boolean } | null = null): HTMLElement {
  const { container } = render(<PlainText text={text} flowed={flowed} links={LINKS} />);
  const root = container.firstElementChild;
  if (!(root instanceof HTMLElement)) {
    throw new TypeError("nothing rendered");
  }
  return root;
}

function depthsOf(root: HTMLElement): (string | null)[] {
  return Array.from(root.querySelectorAll("[data-depth]"), (run) => run.getAttribute("data-depth"));
}

afterEach(cleanup);

test("the lines stand as written, in the text's own direction, with no markup read", () => {
  const root = rendered("Hi,\n\n<b>not bold</b> & done");
  expect(root.getAttribute("dir")).toBe("auto");
  expect(root.textContent).toBe("Hi,\n\n<b>not bold</b> & done");
  expect(root.querySelector("b")).toBeNull();
});

test("quoted lines carry their depth in one quote per level, deeper than three as three", () => {
  const root = rendered("new\n> one\n> still one\n>> two\n>>>> four\nnew again");
  expect(depthsOf(root)).toEqual(["1", "2", "3"]);
  const deepest = root.querySelector('[data-depth="3"]');
  expect(deepest?.textContent).toBe("four");
  // Three quotes around the deepest run, one around the first.
  expect(root.querySelectorAll("blockquote")).toHaveLength(3);
  expect(deepest?.closest("blockquote blockquote blockquote")).toBe(deepest?.parentElement);
  expect(root.querySelector('[data-depth="1"]')?.textContent).toBe("one\nstill one");
  expect(root.querySelector('[data-depth="1"]')?.parentElement?.parentElement).toBe(root);
});

test("a reply inside a quote nests in it and the quote goes on after; unquoted text parts two quotes", () => {
  const nested = rendered("> asked\n>> answered before\n> asked again");
  const outer = nested.querySelectorAll(":scope > blockquote");
  expect(outer).toHaveLength(1);
  expect(outer[0]?.textContent).toBe("askedanswered beforeasked again");
  expect(outer[0]?.querySelector("blockquote")?.textContent).toBe("answered before");
  cleanup();
  const parted = rendered("> first\nbetween\n> second");
  expect(parted.querySelectorAll(":scope > blockquote")).toHaveLength(2);
  expect(parted.querySelectorAll("blockquote blockquote")).toHaveLength(0);
});

test("an empty line that ends a block is drawn and a line keeps the spaces it starts with", () => {
  const root = rendered("para\n\n> quote\n\n indented");
  const blocks = Array.from(root.children, (block) => block.textContent);
  // A line break at a block's end draws nothing, so the empty line rides a second one.
  expect(blocks).toEqual(["para\n\n", "quote", "\n indented"]);
});

test("flowed text comes unfolded into paragraphs with the quote depth kept", () => {
  const root = rendered("> This is a \n> quoted paragraph\nand a fixed line", { delSp: false });
  expect(root.querySelector('[data-depth="1"]')?.textContent).toBe("This is a quoted paragraph");
  expect(root.textContent).toContain("and a fixed line");
});

test("a web address and a mail address become links through the open route with the target as title", () => {
  const root = rendered(
    "See https://shop.example.test/sale?x=1 or www.example.com, write to a@example.com",
  );
  const links = Array.from(root.querySelectorAll("a"));
  expect(links.map((link) => link.textContent)).toEqual([
    "https://shop.example.test/sale?x=1",
    "www.example.com",
    "a@example.com",
  ]);
  for (const link of links) {
    expect(link.getAttribute("href")).toMatch(/^\/open#k=key-1&u=/);
    expect(link.getAttribute("target")).toBe("_blank");
    expect(link.getAttribute("rel")).toBe("noopener noreferrer");
  }
  expect(links[0]?.getAttribute("title")).toBe("https://shop.example.test/sale?x=1");
  expect(links[1]?.getAttribute("title")).toBe("http://www.example.com/");
  expect(links[2]?.getAttribute("title")).toBe("mailto:a@example.com");
  expect(root.textContent).toBe(
    "See https://shop.example.test/sale?x=1 or www.example.com, write to a@example.com",
  );
});

test("an address the policy drops stays text: the app's own host, an API path, another scheme", () => {
  const root = rendered(
    "https://mail.example.test/settings and https://other.example/api/x and ftp://files.example/",
  );
  expect(root.querySelectorAll("a")).toHaveLength(0);
  expect(screen.getByText(/ftp:\/\/files\.example\//)).toBeDefined();
});

test("an address longer than a link can carry stays text", () => {
  const long = `https://shop.example.test/${"a".repeat(5000)}`;
  const root = rendered(`see ${long}`);
  expect(root.querySelectorAll("a")).toHaveLength(0);
  expect(root.textContent).toBe(`see ${long}`);
});

test("a text that changes its quote depth on every line draws a bounded number of blocks and keeps every word", () => {
  const pairs = PLAIN_RUNS_MAX;
  const text = Array.from({ length: pairs }, (_, index) => `plain ${String(index)}\n> quoted`).join(
    "\n",
  );
  const root = rendered(text);
  expect(root.querySelectorAll("div")).toHaveLength(PLAIN_RUNS_MAX);
  // Past the bound the lines stand in one block with their quote marks as written.
  const tail = Array.from(root.children).at(-1);
  expect(tail?.textContent).toContain(`plain ${String(pairs - 1)}\n> quoted`);
  expect(root.textContent).toContain("plain 0");
  expect(root.textContent).toContain(`plain ${String(pairs - 1)}`);
});

test("a text of line breaks at either body cap draws the first lines alone, fixed and flowed", () => {
  const drawn = "\n".repeat(PLAIN_LINES_MAX);
  for (const cap of [BODY_VALUE_BYTES, BODY_VALUE_BYTES_LARGE]) {
    const lines = firstLines("\n".repeat(cap));
    expect(lines).toEqual({ text: drawn, more: true });
    expect(rendered(lines.text).textContent).toBe(drawn);
    cleanup();
    expect(rendered(lines.text, { delSp: false }).textContent).toBe(drawn);
    cleanup();
  }
});

test("a text at the bound of lines stays whole; one line more is cut at the last line break", () => {
  const whole = "line\n".repeat(PLAIN_LINES_MAX);
  expect(firstLines(whole)).toEqual({ text: whole, more: false });
  expect(firstLines(whole.slice(0, -1))).toEqual({ text: whole.slice(0, -1), more: false });
  expect(firstLines(`${whole}past the bound`)).toEqual({ text: whole, more: true });
  expect(firstLines("")).toEqual({ text: "", more: false });
});

test("a text of more addresses than a message may link leaves the rest as text", () => {
  const extra = 5;
  const text = Array.from(
    { length: PLAIN_LINKS_MAX + extra },
    (_, index) => `https://shop.example.test/${String(index)}`,
  ).join("\n> ");
  const root = rendered(text);
  expect(root.querySelectorAll("a")).toHaveLength(PLAIN_LINKS_MAX);
  expect(root.textContent).toContain(
    `https://shop.example.test/${String(PLAIN_LINKS_MAX + extra - 1)}`,
  );
});

test("a text longer than a message may search links the addresses of its whole first lines alone", () => {
  const word = "woord ";
  const address = "https://shop.example.test/a";
  const wordsPerLine = 100;
  const line = `${word.repeat(wordsPerLine)}${address}`;
  // Each line takes its break along.
  const searchedLines = Math.floor(PLAIN_SEARCH_MAX / (line.length + 1));
  const extra = 5;
  const text = Array.from({ length: searchedLines + extra }, () => line).join("\n");
  const root = rendered(text);
  expect(root.querySelectorAll("a")).toHaveLength(searchedLines);
  expect(root.textContent).toBe(text);
  cleanup();
  // One line past the bound has no whole line that fits, so none of it is searched.
  const endless = `${word.repeat(Math.ceil(PLAIN_SEARCH_MAX / word.length))}${address}`;
  expect(rendered(endless).querySelectorAll("a")).toHaveLength(0);
});
