// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { asksDark, namesDark, themedMedia, themedScheme } from "./colors";
import type { Theme } from "./colors";
import type { Images } from "./images";

// A transparent pixel, which stands where a URL may not load.
export const BLANK_PIXEL =
  "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";

const STRINGS = /"[^"]*"|'[^']*'/g;

// The longest declaration value the pass reads, in characters; a longer
// one leaves, so no text test runs over more than this of sender text.
const VALUE_CHARS_MAX = 65_536;

// One url() of a value without escapes: quoted either way, bare or
// empty. Each part matches in one way alone, so the time is linear.
const URL_TOKEN = /url\(\s*(?:"([^"]*)"\s*\)|'([^']*)'\s*\)|([^()"'\s]+)\s*\)|\))/gi;

// The functions beside url() that load what their arguments name.
const LOADER = /(?:^|[^a-z-])(?:-webkit-)?(?:image-set|image|src|cross-fade|element|paint)\(/i;

// A length in a viewport unit, small, large and dynamic ones included,
// or in a container unit, which falls back on the viewport. A digit in
// front of the unit tells it, however the number is written.
const VIEWPORT_LENGTH = /\d(?:[sld]?v|cq)(?:w|h|i|b|min|max)(?![\w-])/i;

// What parts one compound of a selector from the next.
const COMBINATORS = /[\s>+~]+/;

// A compound that names the document's root, its body or everything.
const ROOT_COMPOUND = /^(?:html|body|:root|\*)(?![\w-])/i;

// The properties that size the root, which follows its content here.
const ROOT_HEIGHTS = ["height", "min-height", "max-height"] as const;

// What the CSS of one message is read with and what the read finds.
export interface CssContext {
  images: Images;
  theme: Theme;
  // The URLs this pass wrote, the only ones a block may hold after it.
  written: Set<string>;
  // Whether the message brings styles of its own for the dark scheme.
  declaresDark: boolean;
}

export function cssContext(images: Images, theme: Theme): CssContext {
  return { images, theme, written: new Set([BLANK_PIXEL]), declaresDark: false };
}

// The value without its strings and its url() arguments, so a test of
// its words reads no text the sender quoted.
function words(value: string): string {
  return value.replace(URL_TOKEN, " ").replace(STRINGS, " ");
}

// Whether the selector list names the root or the body as its subject.
export function selectsRoot(selectors: string): boolean {
  return selectors
    .split(",")
    .some((selector) => ROOT_COMPOUND.test(selector.trim().split(COMBINATORS).at(-1) ?? ""));
}

// One url() by the image policy; what may not load becomes the blank
// pixel. A quote inside a data image is written as its percent form,
// so it cannot end the string it is written back in.
function placed(url: string, context: CssContext): string {
  const place = context.images.place(url);
  const kept = place.kind === "loads" ? place.url : place.kind === "data" ? url : BLANK_PIXEL;
  const next = kept.replaceAll('"', "%22");
  context.written.add(next);
  return `url("${next}")`;
}

// The position of a box, which no function may name: a variable or a
// fallback could spell the fixed one the pass demotes.
function positioned(plain: string): string | null {
  if (plain.includes("(")) {
    return null;
  }
  return plain.trim().toLowerCase() === "fixed" ? "absolute" : plain;
}

// The value a declaration keeps, or null for one that leaves: a value
// past the bound or with an escape, which could spell anything; a
// loader beside url(); a viewport length, since the frame is as tall as
// its content.
function cleaned(name: string, value: string, context: CssContext): string | null {
  if (value.length > VALUE_CHARS_MAX || value.includes("\\")) {
    return null;
  }
  const plain = words(value);
  if (LOADER.test(plain) || VIEWPORT_LENGTH.test(plain)) {
    return null;
  }
  if (name === "position") {
    return positioned(value);
  }
  if (name === "color-scheme") {
    context.declaresDark ||= namesDark(value);
    return themedScheme(value, context.theme);
  }
  return value.replace(URL_TOKEN, (_token, double?: string, single?: string, bare?: string) =>
    placed(double ?? single ?? bare ?? "", context),
  );
}

// Whether a block holds nothing the pass did not read: no escape, no
// url() but the ones it wrote and no other loader.
function sound(text: string, written: ReadonlySet<string>): boolean {
  if (text.includes("\\")) {
    return false;
  }
  const rest = text.replace(URL_TOKEN, (token, double?: string, single?: string, bare?: string) =>
    written.has(double ?? single ?? bare ?? "") ? " " : token,
  );
  return !/url\(/i.test(rest) && !LOADER.test(rest.replace(STRINGS, " "));
}

// Cleans one declaration block in place. A block that still holds
// something unread afterwards, as a shorthand with a variable can, is
// emptied whole.
export function cleanBlock(style: CSSStyleDeclaration, context: CssContext): void {
  const names = Array.from({ length: style.length }, (_unused, index) => style.item(index));
  for (const name of names) {
    const value = style.getPropertyValue(name);
    const next = cleaned(name, value, context);
    if (next !== value) {
      const priority = style.getPropertyPriority(name);
      style.removeProperty(name);
      if (next !== null) {
        style.setProperty(name, next, priority);
      }
    }
  }
  if (!sound(style.cssText, context.written)) {
    style.cssText = "";
  }
}

interface RuleList {
  readonly cssRules: CSSRuleList;
  deleteRule(index: number): void;
}

function isRuleList(rule: object): rule is RuleList {
  return "cssRules" in rule && "deleteRule" in rule;
}

// A rule with a selector: its block cleaned, the root's heights dropped
// and the rules nested in it read the same way.
function cleanSelectorRule(rule: CSSStyleRule, context: CssContext): void {
  cleanBlock(rule.style, context);
  if (selectsRoot(rule.selectorText)) {
    for (const name of ROOT_HEIGHTS) {
      rule.style.removeProperty(name);
    }
  }
  if (isRuleList(rule)) {
    cleanRules(rule, context);
  }
}

// Whether a rule stays: one with a selector, a media rule and a
// supports rule do; an import, a font face and every other at-rule
// leave.
function keeps(rule: CSSRule, context: CssContext): boolean {
  if (rule instanceof CSSStyleRule) {
    cleanSelectorRule(rule, context);
    return true;
  }
  if (rule instanceof CSSMediaRule) {
    context.declaresDark ||= asksDark(rule.media.mediaText);
    rule.media.mediaText = themedMedia(rule.media.mediaText, context.theme);
    cleanRules(rule, context);
    return true;
  }
  if (rule instanceof CSSSupportsRule) {
    cleanRules(rule, context);
    return true;
  }
  return false;
}

function cleanRules(parent: RuleList, context: CssContext): void {
  for (let index = parent.cssRules.length - 1; index >= 0; index -= 1) {
    const rule = parent.cssRules.item(index);
    if (rule !== null && !keeps(rule, context)) {
      parent.deleteRule(index);
    }
  }
}

// One block of CSS from a mail, parsed by the engine that will render
// it and cleaned rule by rule. A block with a rule the engine will not
// let go of, as it holds on to a namespace rule, is emptied whole.
export function cleanSheet(source: string, context: CssContext): CSSStyleSheet {
  const sheet = new CSSStyleSheet();
  sheet.replaceSync(source);
  try {
    cleanRules(sheet, context);
    return sheet;
  } catch (error) {
    console.error("mail: a style block was emptied", error instanceof Error ? error.name : "");
    return new CSSStyleSheet();
  }
}

// Every rule with a selector in a sheet, the ones inside a group
// included.
export function selectorRules(parent: { readonly cssRules: CSSRuleList }): CSSStyleRule[] {
  return Array.from(parent.cssRules).flatMap((rule) => {
    const nested = isRuleList(rule) ? selectorRules(rule) : [];
    return rule instanceof CSSStyleRule ? [rule].concat(nested) : nested;
  });
}

// The sheet as the text of its element. A closing tag inside it would
// end that element, so its slash is written as an escape.
export function sheetText(sheet: CSSStyleSheet): string {
  return Array.from(sheet.cssRules, (rule) => rule.cssText)
    .join("\n")
    .replaceAll("</", "<\\/");
}
