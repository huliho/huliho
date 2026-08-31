// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { validateOverride } from "./override";
import type { MergedPalette, StableRole, ThemedValue } from "./palette";
import { parseThemedValue, STABLE_ROLES, tokenName } from "./palette";

const OVERRIDE_URL = "/instance/override.css";

export interface LoadResult {
  applied: boolean;
  message?: string;
}

function readThemeValues(doc: Document, theme: "light" | "dark"): Map<StableRole, string> | null {
  const root = doc.documentElement;
  const previous = root.dataset["theme"];
  // A custom property's computed value keeps light-dark() unresolved, while a
  // build may have compiled it into a resolved value. Each side is read with
  // the theme attribute forced and parsed for either form; the recalculation
  // is synchronous and the attribute is restored before anything paints.
  root.dataset["theme"] = theme;
  const style = getComputedStyle(root);
  const values = new Map<StableRole, string>();
  for (const role of STABLE_ROLES) {
    const raw = style.getPropertyValue(tokenName(role)).trim();
    if (raw !== "") {
      const parsed = parseThemedValue(raw);
      values.set(role, theme === "dark" ? parsed.dark : parsed.light);
    }
  }
  if (previous === undefined) {
    delete root.dataset["theme"];
  } else {
    root.dataset["theme"] = previous;
  }
  return values.size === STABLE_ROLES.length ? values : null;
}

function readBasePalette(doc: Document): MergedPalette | null {
  const light = readThemeValues(doc, "light");
  const dark = readThemeValues(doc, "dark");
  if (light === null || dark === null) {
    return null;
  }
  const palette = new Map<StableRole, ThemedValue>();
  for (const role of STABLE_ROLES) {
    const lightValue = light.get(role);
    const darkValue = dark.get(role);
    if (lightValue === undefined || darkValue === undefined) {
      return null;
    }
    palette.set(role, { light: lightValue, dark: darkValue });
  }
  return palette;
}

function extractDeclarations(sheet: CSSStyleSheet): ReadonlyMap<string, string> | string {
  const declarations = new Map<string, string>();
  for (const rule of sheet.cssRules) {
    // A nested child rule would ride along unvalidated, so any rule holding
    // rules of its own is refused too.
    if (
      !(rule instanceof CSSStyleRule) ||
      rule.selectorText !== ":root" ||
      rule.cssRules.length > 0
    ) {
      return "an override may only contain :root rules with token declarations";
    }
    for (let index = 0; index < rule.style.length; index += 1) {
      const name = rule.style.item(index);
      declarations.set(name, rule.style.getPropertyValue(name).trim());
    }
  }
  return declarations;
}

function toastSheetText(merged: MergedPalette): string {
  const lines: string[] = [];
  for (const role of ["surface", "text", "accent"] as const) {
    const value = merged.get(role);
    if (value !== undefined) {
      lines.push(`--hhx-toast-${role}: light-dark(${value.dark}, ${value.light});`);
    }
  }
  return `:root { ${lines.join(" ")} }`;
}

function rejection(message: string): LoadResult {
  return { applied: false, message };
}

type FetchOutcome =
  { kind: "absent" } | { kind: "failed"; message: string } | { kind: "text"; text: string };

const NOT_FOUND = 404;

async function fetchOverrideText(): Promise<FetchOutcome> {
  let response: Response;
  try {
    response = await fetch(OVERRIDE_URL, { headers: { accept: "text/css" } });
  } catch {
    // Offline is a normal state, not a misconfigured instance.
    return { kind: "absent" };
  }
  const type = response.headers.get("content-type") ?? "";
  if (response.status === NOT_FOUND || (response.ok && !type.includes("text/css"))) {
    // No override mounted: a plain 404 or the fallback page of a static host.
    return { kind: "absent" };
  }
  if (!response.ok) {
    return { kind: "failed", message: `the override answered ${String(response.status)}` };
  }
  return { kind: "text", text: await response.text() };
}

export async function loadInstanceOverride(doc: Document): Promise<LoadResult> {
  const outcome = await fetchOverrideText();
  if (outcome.kind === "absent") {
    return { applied: false };
  }
  if (outcome.kind === "failed") {
    return rejection(outcome.message);
  }
  const text = outcome.text;
  if (text.trim() === "") {
    return { applied: false };
  }
  const sheet = new CSSStyleSheet();
  sheet.replaceSync(text);
  const declarations = extractDeclarations(sheet);
  if (typeof declarations === "string") {
    return rejection(declarations);
  }
  const base = readBasePalette(doc);
  if (base === null) {
    return rejection("the base tokens are not loaded, so the override cannot be checked");
  }
  const verdict = validateOverride(declarations, base);
  if (!verdict.ok) {
    return rejection(verdict.message);
  }
  const toastSheet = new CSSStyleSheet();
  toastSheet.replaceSync(toastSheetText(verdict.merged));
  doc.adoptedStyleSheets = [...doc.adoptedStyleSheets, sheet, toastSheet];
  return { applied: true };
}
