// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { blockedImage } from "./base-style";
import type { FrameStyle } from "./base-style";
import { LEGACY_COLORED, adaptAttributes, adaptBlock } from "./colors";
import { cleanBlock, cleanSheet, selectorRules, selectsRoot, sheetText } from "./css";
import type { CssContext } from "./css";

// One sanitized part of a mail while the passes run over it: its
// elements in a document that loads nothing and its style blocks as the
// engine parsed them.
export interface Piece {
  root: HTMLElement;
  sheets: [HTMLStyleElement, CSSStyleSheet][];
}

// The values of a background that paint nothing; an engine may write
// transparent as its color.
const UNPAINTED = new Set([
  "",
  "none",
  "transparent",
  "rgba(0, 0, 0, 0)",
  "initial",
  "inherit",
  "unset",
]);

const BACKGROUNDS = ["background-color", "background-image", "background"] as const;

function styled(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>("[style]"));
}

// Every image by the policy: one that loads gets its address on this
// instance, a blocked one the box that says so, a dropped one none.
function placeImages(root: HTMLElement, context: CssContext, style: FrameStyle): void {
  for (const image of root.querySelectorAll("img[src]")) {
    const placed = context.images.place(image.getAttribute("src") ?? "");
    if (placed.kind === "loads") {
      image.setAttribute("src", placed.url);
    } else if (placed.kind === "blocked") {
      image.setAttribute("src", blockedImage(image.getAttribute("alt") ?? "", style));
    } else if (placed.kind === "dropped") {
      image.removeAttribute("src");
    }
  }
}

// A background attribute by the same policy; a blocked one leaves, since
// a cell without its background still reads.
function placeBackgrounds(root: HTMLElement, context: CssContext): void {
  for (const cell of root.querySelectorAll("[background]")) {
    const placed = context.images.place(cell.getAttribute("background") ?? "");
    if (placed.kind === "loads") {
      cell.setAttribute("background", placed.url);
    } else if (placed.kind !== "data") {
      cell.removeAttribute("background");
    }
  }
}

// A wrapper as tall as the frame would never let the frame shrink to
// its content, so a top element loses a height in percent.
function unpinHeight(element: Element): void {
  if (element.getAttribute("height")?.trim().endsWith("%") === true) {
    element.removeAttribute("height");
  }
  if (!(element instanceof HTMLElement)) {
    return;
  }
  for (const name of ["height", "min-height"]) {
    if (element.style.getPropertyValue(name).includes("%")) {
      element.style.removeProperty(name);
    }
  }
}

// The first pass over one sanitized part: its CSS cleaned, its images
// placed by the policy and its top elements freed of a full height.
export function cleanPiece(root: HTMLElement, context: CssContext, style: FrameStyle): Piece {
  const sheets = Array.from(root.querySelectorAll("style"), (element): Piece["sheets"][number] => [
    element,
    cleanSheet(element.textContent, context),
  ]);
  for (const element of styled(root)) {
    cleanBlock(element.style, context);
  }
  placeImages(root, context, style);
  placeBackgrounds(root, context);
  for (const element of Array.from(root.children)) {
    unpinHeight(element);
  }
  return { root, sheets };
}

// The second pass, for a light-only mail in the dark theme: every color
// its CSS and its legacy attributes set, turned around.
export function adaptPiece({ root, sheets }: Piece): void {
  for (const [, sheet] of sheets) {
    for (const rule of selectorRules(sheet)) {
      adaptBlock(rule.style);
    }
  }
  for (const element of styled(root)) {
    adaptBlock(element.style);
  }
  for (const element of root.querySelectorAll<HTMLElement>(LEGACY_COLORED)) {
    adaptAttributes(element);
  }
}

function paints(style: CSSStyleDeclaration): boolean {
  return BACKGROUNDS.some(
    (name) => !UNPAINTED.has(style.getPropertyValue(name).trim().toLowerCase()),
  );
}

function matches(element: Element, selectors: string): boolean {
  try {
    return element.matches(selectors);
  } catch {
    // A selector this engine cannot read matches nothing here either.
    return false;
  }
}

// Whether the mail paints a background of its own on the root, the
// body or the one element that wraps it, so the frame adds no room
// around it.
export function paintsBackground({ root, sheets }: Piece): boolean {
  const tops = Array.from(root.children).filter((element) => element.localName !== "style");
  const outer = tops.length === 1 ? (tops[0] ?? null) : null;
  const wraps = (selectors: string): boolean => outer !== null && matches(outer, selectors);
  const byRule = sheets
    .flatMap(([, sheet]) => selectorRules(sheet))
    .some(
      (rule) => paints(rule.style) && (selectsRoot(rule.selectorText) || wraps(rule.selectorText)),
    );
  if (byRule || outer === null) {
    return byRule;
  }
  const byAttribute = outer.hasAttribute("bgcolor") || outer.hasAttribute("background");
  return byAttribute || (outer instanceof HTMLElement && paints(outer.style));
}

// The part as HTML once the passes ran: every style block and style
// attribute written back as the engine reads it, so nothing it did not
// parse reaches the frame.
export function pieceHtml({ root, sheets }: Piece): string {
  for (const [element, sheet] of sheets) {
    element.textContent = sheetText(sheet);
  }
  for (const element of styled(root)) {
    const text = element.style.cssText;
    if (text === "") {
      element.removeAttribute("style");
    } else {
      element.setAttribute("style", text);
    }
  }
  return root.innerHTML;
}
