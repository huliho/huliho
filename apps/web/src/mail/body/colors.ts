// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

// The theme the app renders, which decides the scheme inside the frame
// whatever the operating system prefers.
export type Theme = "light" | "dark";

const SCHEME_FEATURE = /\(\s*prefers-color-scheme\s*:\s*(dark|light)\s*\)/gi;
const DARK_FEATURE = /prefers-color-scheme\s*:\s*dark/i;

// A media feature that holds in every frame and one that holds in none.
const ALWAYS = "(min-width: 0px)";
const NEVER = "(not (min-width: 0px))";

// A media query with every color scheme feature settled by the theme:
// the scheme the app shows always holds and the other one never does.
export function themedMedia(media: string, theme: Theme): string {
  return media.replace(SCHEME_FEATURE, (_feature, scheme: string) =>
    scheme.toLowerCase() === theme ? ALWAYS : NEVER,
  );
}

// Whether a media query holds styles for the dark scheme.
export function asksDark(media: string): boolean {
  return DARK_FEATURE.test(media);
}

// Whether a color-scheme value names the dark scheme.
export function namesDark(scheme: string): boolean {
  return /\bdark\b/i.test(scheme);
}

// A color-scheme value that offers both schemes becomes the theme's
// own, so the frame's canvas follows the app and not the system.
export function themedScheme(scheme: string, theme: Theme): string {
  return namesDark(scheme) && /\blight\b/i.test(scheme) ? theme : scheme;
}

// The color properties the adaptation rewrites, by their longhands: a
// shorthand's color reads through them.
const COLOR_PROPERTIES = [
  "color",
  "background-color",
  "border-top-color",
  "border-right-color",
  "border-bottom-color",
  "border-left-color",
  "outline-color",
  "text-decoration-color",
] as const;

// The values that name no color of their own.
const UNTOUCHED = new Set([
  "",
  "transparent",
  "currentcolor",
  "inherit",
  "initial",
  "unset",
  "revert",
  "revert-layer",
]);

const ADAPTED_PREFIX = "oklch(from ";

// A legacy color attribute without its hash sign (three or six digits).
const BARE_HEX = /^(?:[0-9a-f]{3}|[0-9a-f]{6})$/i;

// The color with its lightness turned around: white becomes black, a
// near-black text becomes near-white and a saturated color keeps its
// hue and chroma.
function turned(color: string): string {
  return `${ADAPTED_PREFIX}${color} calc(1 - l) c h / alpha)`;
}

// The same test the rewrite's own syntax has to pass in this engine.
const SUPPORT_PROBE = turned("red");

// Whether this engine can adapt: it reads the relative color syntax and
// the reader has not asked for forced colors.
export function canAdapt(): boolean {
  return CSS.supports("color", SUPPORT_PROBE) && !matchMedia("(forced-colors: active)").matches;
}

function adaptProperty(style: CSSStyleDeclaration, name: string): void {
  const value = style.getPropertyValue(name).trim();
  const plain = value.toLowerCase();
  if (UNTOUCHED.has(plain) || plain.startsWith(ADAPTED_PREFIX)) {
    return;
  }
  // An engine that refuses the value keeps the color it had.
  style.setProperty(name, turned(value), style.getPropertyPriority(name));
}

// Adapts every color of one declaration block for the dark theme. A
// color inside a gradient or another image is left alone.
export function adaptBlock(style: CSSStyleDeclaration): void {
  for (const name of COLOR_PROPERTIES) {
    adaptProperty(style, name);
  }
}

// The elements a browser paints a legacy color attribute on: the parts
// of a table take `bgcolor`, a font element takes `color`.
export const LEGACY_COLORED =
  "table[bgcolor], thead[bgcolor], tbody[bgcolor], tfoot[bgcolor], tr[bgcolor], " +
  "td[bgcolor], th[bgcolor], font[color]";

// A legacy color the adaptation takes: a name or hex digits. Anything
// else is sender text that no check read and stays out of the style.
const LEGACY_COLOR = /^(?:#?(?:[0-9a-f]{3}|[0-9a-f]{6})|[a-z]+)$/i;

// The color of a legacy attribute as CSS reads it, null for a value
// that is no plain color.
function legacyColor(value: string): string | null {
  const plain = value.trim();
  if (!LEGACY_COLOR.test(plain)) {
    return null;
  }
  return BARE_HEX.test(plain) ? `#${plain}` : plain;
}

// Adapts the legacy color attribute of one such element through its
// style, which outranks it; a color the style sets itself stays the
// style's.
export function adaptAttributes(element: HTMLElement): void {
  const [attribute, property] =
    element.localName === "font" ? ["color", "color"] : ["bgcolor", "background-color"];
  const color = legacyColor(element.getAttribute(attribute) ?? "");
  if (color !== null && element.style.getPropertyValue(property) === "") {
    element.style.setProperty(property, turned(color));
  }
}
