// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { expect, test, vi } from "vitest";

import type { Theme } from "./colors";
import { BLANK_PIXEL, cleanBlock, cleanSheet, cssContext, selectsRoot, sheetText } from "./css";
import type { CssContext } from "./css";
import { DOWNLOAD, DOWNLOAD_PREFIX, OWN_HOST, PROXY_PREFIX, message, part } from "./frame-rig";
import { Images } from "./images";

const LOGO = part("image/png", { blobId: "b-logo", name: "logo.png", cid: "logo@shop.example" });
const PIXEL = `url("${BLANK_PIXEL}")`;

function context(remote = false, theme: Theme = "light"): CssContext {
  const images = new Images(message([], [LOGO]), { ownHost: OWN_HOST, remote, download: DOWNLOAD });
  return cssContext(images, theme);
}

// A style block as the frame gets it.
function sheet(source: string, read = context()): string {
  return sheetText(cleanSheet(source, read));
}

// A style attribute as the frame gets it.
function inline(source: string, read = context()): string {
  const page = new DOMParser().parseFromString(`<div style="${source}">x</div>`, "text/html");
  const element = page.body.querySelector("div");
  if (element === null) {
    throw new Error("the fixture holds no element");
  }
  cleanBlock(element.style, read);
  return element.style.cssText;
}

test("an import, a font face and every other at-rule leave a style block", () => {
  const output = sheet(
    '@import url("https://evil.example/steal.css"); @font-face { font-family: x; src: url(https://evil.example/f.woff) }' +
      " @keyframes spin { from { color: red } } @layer base { p { color: red } }" +
      " p { color: red }",
  );
  expect(output).toBe("p { color: red; }");
});

test("a block with a rule the engine will not let go of is emptied whole and named", () => {
  const logged = vi.spyOn(console, "error").mockImplementation(() => undefined);
  const source = "@namespace svg url(http://www.w3.org/2000/svg); p { color: red }";
  expect(sheet(source)).toBe("");
  expect(logged).toHaveBeenCalledExactlyOnceWith(
    "mail: a style block was emptied",
    expect.any(String),
  );
  logged.mockRestore();
});

test("a remote image in a block is a blank pixel until the reader allows the sender", () => {
  const blocked = context();
  expect(sheet("p { background-image: url(https://evil.example/pixel?x=1) }", blocked)).toBe(
    `p { background-image: ${PIXEL}; }`,
  );
  expect(blocked.images.remote).toBe(1);
  const allowed = context(true);
  expect(sheet("p { background-image: url('https://cdn.example/bg.png') }", allowed)).toBe(
    `p { background-image: url("${PROXY_PREFIX}https%3A%2F%2Fcdn.example%2Fbg.png"); }`,
  );
  expect(allowed.images.remote).toBe(1);
});

test("the same remote address counts once, however often it is named", () => {
  const read = context();
  sheet(
    "p { background-image: url(https://a.example/1.png) } b { background-image: url(https://a.example/1.png) }",
    read,
  );
  inline("background-image: url(https://a.example/1.png)", read);
  inline("background-image: url(https://a.example/2.png)", read);
  expect(read.images.remote).toBe(2);
});

test("a part of the message loads from the download route and a data image stays", () => {
  expect(inline("background-image: url(cid:logo@shop.example)")).toBe(
    `background-image: url("${DOWNLOAD_PREFIX}b-logo/logo.png?type=image%2Fpng");`,
  );
  expect(inline("background-image: url(data:image/png;base64,iVBORw0KGgo=)")).toBe(
    'background-image: url("data:image/png;base64,iVBORw0KGgo=");',
  );
});

test.each([
  "cid:other@message.example",
  "/api/remote-image?url=https%3A%2F%2Fevil.example%2Fp.gif",
  "https://mail.example.test/api/jmap/a1/download/u1/b1/x.png?type=image/png",
  "https://mail.example.test/logo.png",
  "logo.png",
  "//evil.example/x.png",
  "data:text/html,x",
  "javascript:top.__x=1",
])("a URL the policy drops becomes the blank pixel: %s", (url) => {
  const read = context(true);
  expect(inline(`background-image: url(${url})`, read)).toBe(`background-image: ${PIXEL};`);
  expect(read.images.remote).toBe(0);
});

test.each([
  ["height: 100vh", ""],
  ["width: 50VW", ""],
  ["min-height: calc(100dvh - 20px)", ""],
  ["font-size: 4vmin", ""],
  ["margin-top: -1.5svh", ""],
  ["min-height: 100cqh", ""],
  ["inline-size: .5cqmin", ""],
  ["--tall: 1e2vh", ""],
  ["width: 50%", "width: 50%;"],
  ["width: 5ch", "width: 5ch;"],
  ["position: fixed", "position: absolute;"],
  ["position: FIXED !important", "position: absolute !important;"],
  ["position: sticky", "position: sticky;"],
])("a viewport length leaves and a fixed position is demoted: %s", (source, output) => {
  expect(inline(source)).toBe(output);
});

test("a position that a function names leaves, since it could spell the fixed one", () => {
  const output = sheet(
    ":root { --p: fixed } a { position: var(--p); color: red }" +
      " b { position: var(--none, fixed); color: red } i { position: relative }",
  );
  expect(output).not.toContain("position: var");
  expect(output).toContain("--p: fixed");
  expect(output).toContain("position: relative");
  expect(output.match(/color: red/g)).toHaveLength(2);
});

// A value just under the longest the pass reads and one past it.
const LONG_CHARS = 60_000;
const TOO_LONG_CHARS = 70_000;
// Enough reads of a long value that a pattern with quadratic time does
// not finish inside a test's time, while a linear one is done at once.
const READS = 40;

// The style of one element whose custom property reads back as `value`,
// as a browser hands back what the sender wrote.
function holding(value: string): CSSStyleDeclaration {
  const page = new DOMParser().parseFromString(
    '<div style="--x: 1; color: red">x</div>',
    "text/html",
  );
  const element = page.body.querySelector("div");
  if (element === null) {
    throw new Error("the fixture holds no element");
  }
  const real = element.style.getPropertyValue.bind(element.style);
  vi.spyOn(element.style, "getPropertyValue").mockImplementation((name) =>
    name === "--x" ? value : real(name),
  );
  return element.style;
}

test.each([
  ["a run of digits", "1".repeat(LONG_CHARS)],
  ["a number with a long fraction", `1.${"1".repeat(LONG_CHARS)}`],
  ["url( and a run of spaces inside a string", `"url(${" ".repeat(LONG_CHARS)}"`],
  ["url( and a run of spaces before a name", `url(${" ".repeat(LONG_CHARS)}x`],
  ["url( over and over", "url(".repeat(LONG_CHARS / "url(".length)],
])("a long value is read in linear time: %s", (_what, value) => {
  for (let read = 0; read < READS; read += 1) {
    const style = holding(value);
    cleanBlock(style, context());
    expect(style.getPropertyValue("color")).toBe("red");
  }
});

test("a value past the longest the pass reads leaves its declaration", () => {
  const style = holding(`${"a".repeat(TOO_LONG_CHARS)} url(https://evil.example/x.png)`);
  const removed = vi.spyOn(style, "removeProperty");
  cleanBlock(style, context());
  expect(removed).toHaveBeenCalledExactlyOnceWith("--x");
  expect(style.getPropertyValue("color")).toBe("red");
});

test("a quote inside a data image cannot end the string it is written back in", () => {
  const read = context();
  const page = new DOMParser().parseFromString('<div style="--bg: red">x</div>', "text/html");
  const element = page.body.querySelector("div");
  if (element === null) {
    throw new Error("the fixture holds no element");
  }
  // A browser hands a custom property back as it was written, with the
  // sender's own quotes.
  const written = `url('data:image/png,A") url("https://evil.example/x.png')`;
  vi.spyOn(element.style, "getPropertyValue").mockReturnValueOnce(written);
  const set = vi.spyOn(element.style, "setProperty");
  cleanBlock(element.style, read);
  expect(set).toHaveBeenCalledExactlyOnceWith(
    "--bg",
    'url("data:image/png,A%22) url(%22https://evil.example/x.png")',
    "",
  );
  expect(read.images.remote).toBe(0);
});

test("a loader beside url() and a value with an escape take their declaration along", () => {
  const output = sheet(
    'a { background-image: image-set("https://evil.example/a.png" 1x); color: red }' +
      " b { background-image: -webkit-image-set(url(https://evil.example/b.png) 1x); color: red }" +
      " i { background-image: u\\72l(https://evil.example/c.png); color: red }" +
      " u { --bg: u\\72l(https://evil.example/d.png); color: red }",
  );
  expect(output).not.toContain("evil.example");
  expect(output.match(/color: red/g)).toHaveLength(4);
});

test("a custom property is read like any other value", () => {
  const read = context();
  const output = sheet(":root { --bg: url(https://evil.example/e.png); --gap: 4px }", read);
  expect(output).toContain(`--bg: ${PIXEL}`);
  expect(output).toContain("--gap: 4px");
  expect(read.images.remote).toBe(1);
});

test("a block that still holds a URL the pass did not write is emptied whole", () => {
  const read = context();
  const page = new DOMParser().parseFromString("<div>x</div>", "text/html");
  const element = page.body.querySelector("div");
  if (element === null) {
    throw new Error("the fixture holds no element");
  }
  // What an engine keeps of a shorthand with a variable: a value no
  // longhand shows.
  Object.defineProperty(element.style, "cssText", {
    configurable: true,
    get: () => "color: red; background: url(https://evil.example/x.png) var(--y);",
    set: (text: string) => {
      Reflect.deleteProperty(element.style, "cssText");
      element.style.cssText = text;
    },
  });
  element.style.setProperty("color", "red");
  cleanBlock(element.style, read);
  expect(element.style.cssText).toBe("");
});

test("the root and the body lose every height a sender gives them", () => {
  const output = sheet(
    "html, body { height: 100%; margin: 0 } body.mail { min-height: 100%; max-height: 400px; color: red }" +
      " * { height: 100% } div { height: 100% } html > body .x { height: 50% }",
  );
  expect(output).toBe(
    [
      "html, body { margin: 0px; }",
      "body.mail { color: red; }",
      "* { }",
      "div { height: 100%; }",
      "html > body .x { height: 50%; }",
    ].join("\n"),
  );
});

test.each([
  ["html", true],
  ["BODY", true],
  [":root", true],
  ["*", true],
  ["table, body", true],
  ["html > body", true],
  ["body.dark", true],
  ["body#top", true],
  ["body[data-x]", true],
  ["body:hover", true],
  [".body", false],
  ["bodyguard", false],
  ["body table", false],
  ["html-like", false],
  ["#html", false],
])("the subject of %s is the root: %s", (selectors, answer) => {
  expect(selectsRoot(selectors)).toBe(answer);
});

test("a media query's color scheme is settled by the app's theme", () => {
  const source =
    "@media (prefers-color-scheme: dark) { p { color: white } }" +
    " @media screen and (prefers-color-scheme: light) and (max-width: 600px) { p { color: black } }" +
    " @media (max-width: 600px) { p { color: gray } }";
  const dark = context(false, "dark");
  expect(sheet(source, dark)).toBe(
    [
      "@media (min-width: 0px) {\n  p { color: white; }\n}",
      "@media screen and (not (min-width: 0px)) and (max-width: 600px) {\n  p { color: black; }\n}",
      "@media (max-width: 600px) {\n  p { color: gray; }\n}",
    ].join("\n"),
  );
  expect(dark.declaresDark).toBe(true);
  const light = context(false, "light");
  expect(sheet(source, light)).toContain("@media (not (min-width: 0px)) {");
  expect(sheet(source, light)).toContain("screen and (min-width: 0px) and (max-width: 600px)");
  expect(light.declaresDark).toBe(true);
});

test("a mail without a dark query or scheme declares none", () => {
  const read = context(false, "dark");
  sheet(
    "@media (prefers-color-scheme: light) { p { color: black } } p { color-scheme: light }",
    read,
  );
  inline("color-scheme: only light", read);
  expect(read.declaresDark).toBe(false);
});

test("a color-scheme that offers both becomes the theme's own and one that names dark is declared", () => {
  const dark = context(false, "dark");
  expect(sheet(":root { color-scheme: light dark }", dark)).toBe(":root { color-scheme: dark; }");
  expect(dark.declaresDark).toBe(true);
  const light = context(false, "light");
  expect(inline("color-scheme: light dark", light)).toBe("color-scheme: light;");
  expect(light.declaresDark).toBe(true);
  expect(inline("color-scheme: dark", context(false, "light"))).toBe("color-scheme: dark;");
});

test("the rules inside a media rule and a supports rule are read the same way", () => {
  const read = context();
  const output = sheet(
    "@media (max-width: 600px) { @supports (display: grid) { p { background-image: url(https://evil.example/n.png); height: 100vh } @font-face { font-family: x } } }",
    read,
  );
  expect(output).not.toContain("evil.example");
  expect(output).not.toContain("100vh");
  expect(output).not.toContain("@font-face");
  expect(output).toContain(PIXEL);
  expect(read.images.remote).toBe(1);
});

test("a closing tag inside a style block cannot end its element", () => {
  const output = sheet('p::after { content: "</style><img src=x onerror=top.__x=1>" }');
  expect(output).not.toContain("</style");
  expect(output).toContain("<\\/style");
});
