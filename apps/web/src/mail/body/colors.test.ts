// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { afterEach, expect, test, vi } from "vitest";

import {
  LEGACY_COLORED,
  adaptAttributes,
  adaptBlock,
  asksDark,
  canAdapt,
  namesDark,
  themedMedia,
  themedScheme,
} from "./colors";

function element(html: string): HTMLElement {
  const page = new DOMParser().parseFromString(html, "text/html");
  const found = page.body.firstElementChild;
  if (!(found instanceof HTMLElement)) {
    throw new Error("the fixture holds no element");
  }
  return found;
}

function turned(color: string): string {
  return `oklch(from ${color} calc(1 - l) c h / alpha)`;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test.each([
  ["(prefers-color-scheme: dark)", "dark", "(min-width: 0px)"],
  ["(prefers-color-scheme: dark)", "light", "(not (min-width: 0px))"],
  ["(prefers-color-scheme:light)", "light", "(min-width: 0px)"],
  ["( PREFERS-COLOR-SCHEME : DARK )", "dark", "(min-width: 0px)"],
  [
    "screen and (prefers-color-scheme: dark) and (max-width: 600px)",
    "light",
    "screen and (not (min-width: 0px)) and (max-width: 600px)",
  ],
  [
    "(prefers-color-scheme: dark), (prefers-color-scheme: light)",
    "dark",
    "(min-width: 0px), (not (min-width: 0px))",
  ],
  ["not (prefers-color-scheme: dark)", "dark", "not (min-width: 0px)"],
  ["(max-width: 600px)", "dark", "(max-width: 600px)"],
] as const)("the theme settles the scheme of %s under %s", (media, theme, output) => {
  expect(themedMedia(media, theme)).toBe(output);
});

test("a query for the dark scheme and a scheme that names dark are told", () => {
  expect(asksDark("screen and (prefers-color-scheme:dark)")).toBe(true);
  expect(asksDark("(prefers-color-scheme: light)")).toBe(false);
  expect(namesDark("light dark")).toBe(true);
  expect(namesDark("only DARK")).toBe(true);
  expect(namesDark("light")).toBe(false);
  expect(namesDark("darkish")).toBe(false);
});

test("a scheme that offers both becomes the theme's and any other stays", () => {
  expect(themedScheme("light dark", "dark")).toBe("dark");
  expect(themedScheme("dark light", "light")).toBe("light");
  expect(themedScheme("dark", "light")).toBe("dark");
  expect(themedScheme("only light", "dark")).toBe("only light");
  expect(themedScheme("normal", "dark")).toBe("normal");
});

test("every color of a block is turned around and its importance kept", () => {
  const { style } = element(
    '<p style="color: #232b2f; background: #ffffff; border: 1px solid #cccccc; outline-color: red !important; text-decoration-color: blue">x</p>',
  );
  adaptBlock(style);
  expect(style.getPropertyValue("color")).toBe(turned("rgb(35, 43, 47)"));
  expect(style.getPropertyValue("background-color")).toBe(turned("rgb(255, 255, 255)"));
  expect(style.getPropertyValue("border-left-color")).toBe(turned("rgb(204, 204, 204)"));
  expect(style.getPropertyValue("outline-color")).toBe(turned("red"));
  expect(style.getPropertyPriority("outline-color")).toBe("important");
  expect(style.getPropertyValue("text-decoration-color")).toBe(turned("blue"));
});

test("a value that names no color of its own, an image and a color already turned stay", () => {
  const source =
    "color: inherit; background-color: transparent; outline-color: currentcolor; " +
    "border-top-color: initial; background-image: linear-gradient(#ffffff, #000000)";
  const { style } = element(`<p style="${source}">x</p>`);
  const before = style.cssText;
  adaptBlock(style);
  expect(style.cssText).toBe(before);
  const twice = element('<p style="color: #ffffff">x</p>').style;
  adaptBlock(twice);
  const once = twice.cssText;
  adaptBlock(twice);
  expect(twice.cssText).toBe(once);
});

// One element with one attribute, made without the parser, which would
// move a table cell out of a body.
function made(name: string, attribute: string, value: string): HTMLElement {
  const created = document.createElement(name);
  created.setAttribute(attribute, value);
  return created;
}

test("a legacy color attribute is adapted through the style, with its hash sign", () => {
  const cell = made("td", "bgcolor", "FF0000");
  adaptAttributes(cell);
  expect(cell.style.getPropertyValue("background-color")).toBe(turned("rgb(255, 0, 0)"));
  expect(cell.getAttribute("bgcolor")).toBe("FF0000");
  const short = made("font", "color", "#333");
  adaptAttributes(short);
  expect(short.style.getPropertyValue("color")).toBe(turned("rgb(51, 51, 51)"));
  const named = made("font", "color", " Red ");
  adaptAttributes(named);
  expect(named.style.getPropertyValue("color")).toBe(turned("red"));
});

test.each([
  ["td", "bgcolor", "var(--a) url(https://probe.invalid/leak.png)"],
  ["font", "color", "var(--a) url(https://probe.invalid/leak.png)"],
  ["td", "bgcolor", "rgb(1, 2, 3)"],
  ["td", "bgcolor", "red; background: url(https://probe.invalid/x.png)"],
  ["font", "color", "#12345"],
  ["font", "color", ""],
])(
  "a legacy attribute that is no plain color stays out of the style: %s %s=%j",
  (name, attribute, value) => {
    const subject = made(name, attribute, value);
    adaptAttributes(subject);
    expect(subject.getAttribute("style")).toBeNull();
  },
);

test("a legacy color is adapted where a browser paints it and nowhere else", () => {
  const page = new DOMParser().parseFromString(
    '<div bgcolor="#ffffff"><p color="red">x</p><font color="red" bgcolor="#ffffff">y</font>' +
      '<table bgcolor="#ffffff"><tr><td bgcolor="#eeeeee" color="red">z</td></tr></table></div>',
    "text/html",
  );
  const painted = Array.from(page.body.querySelectorAll<HTMLElement>(LEGACY_COLORED));
  expect(painted.map((subject) => subject.localName)).toEqual(["font", "table", "td"]);
  for (const subject of painted) {
    adaptAttributes(subject);
  }
  expect(painted.map((subject) => subject.style.cssText)).toEqual([
    `color: ${turned("red")};`,
    `background-color: ${turned("rgb(255, 255, 255)")};`,
    `background-color: ${turned("rgb(238, 238, 238)")};`,
  ]);
});

test("a color the style sets itself stays the style's", () => {
  const cell = element('<font color="#ffffff" style="color: rgb(1, 2, 3)">x</font>');
  adaptAttributes(cell);
  expect(cell.style.getPropertyValue("color")).toBe("rgb(1, 2, 3)");
});

test("the adaptation needs the relative color syntax and no forced colors", () => {
  const supports = vi.fn<(property: string, value: string) => boolean>(() => true);
  const forced = { matches: false };
  vi.stubGlobal("CSS", { supports });
  vi.stubGlobal("matchMedia", () => forced);
  expect(canAdapt()).toBe(true);
  expect(supports).toHaveBeenCalledWith("color", turned("red"));
  forced.matches = true;
  expect(canAdapt()).toBe(false);
  forced.matches = false;
  supports.mockReturnValue(false);
  expect(canAdapt()).toBe(false);
});
