// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { readFileSync } from "node:fs";
import { join } from "node:path";

import { expect, test } from "vitest";

import { GENERIC_ATTRIBUTES, LINK_REL, LINK_TARGET, TAGS, URL_ATTRIBUTES, purify } from "./profile";

// The server's layer of the sanitizer, whose lists this layer mirrors.
const SERVER = readFileSync(
  join(import.meta.dirname, "../../../../../crates/server/src/mail/sanitize.rs"),
  "utf8",
);

// The text between the brackets of one constant of the server's source.
function constant(name: string): string {
  const start = SERVER.indexOf(`pub const ${name}:`);
  const open = SERVER.indexOf("= &[", start);
  const close = SERVER.indexOf("];", open);
  expect(start).toBeGreaterThan(-1);
  return SERVER.slice(open, close);
}

function quoted(text: string): string[] {
  return Array.from(text.matchAll(/"([^"]+)"/g), ([, word]) => word ?? "");
}

function clean(html: string): string {
  return purify(html).innerHTML;
}

test("the profile names the elements and attributes the server's sanitizer names", () => {
  expect([...TAGS]).toEqual(quoted(constant("TAGS")));
  expect([...GENERIC_ATTRIBUTES]).toEqual(quoted(constant("GENERIC_ATTRIBUTES")));
  expect(URL_ATTRIBUTES.flat()).toEqual(quoted(constant("URL_ATTRIBUTES")));
  expect(SERVER).toContain(`pub const LINK_TARGET: &str = "${LINK_TARGET}";`);
  expect(SERVER).toContain(`pub const LINK_REL: &str = "${LINK_REL}";`);
});

test("the body comes back in a document of its own, which runs and loads nothing", () => {
  const body = purify("<p>hi</p>");
  expect(body.localName).toBe("body");
  expect(body.ownerDocument).not.toBe(document);
  expect(body.ownerDocument.defaultView).toBeNull();
});

test.each([
  ["<script>top.__x=1</script><p>a</p>", "<p>a</p>"],
  ['<iframe src="https://evil.example/"></iframe><p>a</p>', "<p>a</p>"],
  ['<form action="https://evil.example/"><input name="p"><button>Go</button></form>', "Go"],
  ['<p onclick="top.__x=1" ONMOUSEOVER="top.__x=1">a</p>', "<p>a</p>"],
  [
    '<img src="https://cdn.example/a.png" srcset="https://evil.example/t.png 1x">',
    '<img src="https://cdn.example/a.png">',
  ],
  ['<p data-x="1" aria-label="q" name="body" role="button" tabindex="0">a</p>', "<p>a</p>"],
  ["<svg><script>top.__x=1</script></svg><math><mi>x</mi></math><p>a</p>", "<p>a</p>"],
  [
    '<meta http-equiv="refresh" content="0"><base href="https://evil.example/"><p>a</p>',
    "<p>a</p>",
  ],
  ['<link rel="stylesheet" href="https://evil.example/x.css"><p>a</p>', "<p>a</p>"],
  ["<p>a<!-- a comment --></p>", "<p>a</p>"],
])("what the allowlist does not name leaves: %s", (html, output) => {
  expect(clean(html)).toBe(output);
});

test("a style block and the attributes of a mail's layout stay", () => {
  const html =
    '<style>p{color:red}</style><table width="600" cellpadding="0" bgcolor="#ffffff">' +
    '<tbody><tr><td align="center" valign="top" style="color:blue" class="c" id="i" dir="rtl" lang="nl" title="t">' +
    '<font face="Arial" size="2" color="#333333">x</font></td></tr></tbody></table>';
  expect(clean(html)).toBe(html);
});

test("a URL attribute stays on the elements that carry it and nowhere else", () => {
  const output = clean(
    '<p href="https://x.example/" src="https://x.example/a.png" background="https://x.example/b.png">a</p>' +
      '<div background="https://x.example/b.png">b</div><img href="https://x.example/">' +
      '<table background="https://x.example/t.png"><tbody><tr background="https://x.example/r.png">' +
      '<td background="https://x.example/d.png">c</td></tr></tbody></table>',
  );
  expect(output).toBe(
    "<p>a</p><div>b</div><img>" +
      '<table background="https://x.example/t.png"><tbody><tr background="https://x.example/r.png">' +
      '<td background="https://x.example/d.png">c</td></tr></tbody></table>',
  );
});

test("every link loses the sender's target and rel and gets its own", () => {
  const output = purify('<a href="https://x.example/" target="_top" rel="opener" ping="/t">x</a>');
  const link = output.querySelector("a");
  expect(link?.getAttribute("target")).toBe("_blank");
  expect(link?.getAttribute("rel")).toBe("noopener noreferrer");
  expect(link?.hasAttribute("ping")).toBe(false);
  expect(purify('<p target="_top" rel="x">a</p>').innerHTML).toBe("<p>a</p>");
});

test.each([
  ['<a href="javascript:top.__x=1">x</a>', "href"],
  ['<a href=" JAVASCRIPT:top.__x=1">x</a>', "href"],
  ['<a href="java&#9;script:top.__x=1">x</a>', "href"],
  ['<a href="data:text/html,<script>top.__x=1</script>">x</a>', "href"],
  ['<a href="vbscript:x">x</a>', "href"],
  ['<a href="file:///etc/passwd">x</a>', "href"],
  ['<img src="javascript:top.__x=1">', "src"],
  ['<img src="data:text/html,<script>top.__x=1</script>">', "src"],
  ['<img src="data:text/html,x">', "src"],
  ['<img src=" DATA:text/html,x">', "src"],
  ['<img src="data:,x">', "src"],
  ['<table background="javascript:top.__x=1"></table>', "background"],
  ['<table background="data:text/html,x"></table>', "background"],
])("a scheme off the list takes its attribute along: %s", (html, attribute) => {
  expect(purify(html).querySelector(`[${attribute}]`)).toBeNull();
});

test.each([
  ['<a href="https://x.example/a?b=c#d">x</a>', "href"],
  ['<a href="mailto:sanne@example.test">x</a>', "href"],
  ['<img src="cid:part1@shop.example">', "src"],
  ['<img src="data:image/png;base64,iVBORw0KGgo=">', "src"],
  ['<table background="data:image/png;base64,iVBORw0KGgo="></table>', "background"],
])("a scheme on the list stays for the policy to settle: %s", (html, attribute) => {
  expect(purify(html).querySelector(`[${attribute}]`)).not.toBeNull();
});
