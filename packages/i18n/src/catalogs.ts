// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { MessageFormatElement } from "@formatjs/icu-messageformat-parser";
import {
  isDateElement,
  isLiteralElement,
  isPluralElement,
  isSelectElement,
  isTagElement,
  isTimeElement,
  parse,
} from "@formatjs/icu-messageformat-parser";
import { printAST } from "@formatjs/icu-messageformat-parser/printer.js";

export type Catalog = Readonly<Record<string, string>>;

export interface ProjectSettings {
  baseLocale: string;
  locales: string[];
}

// The pseudo catalog is generated from the base locale at build time.
export const PSEUDO_LOCALE = "en-XA";

export function parseSettings(value: unknown, source: string): ProjectSettings {
  if (
    typeof value === "object" &&
    value !== null &&
    "baseLocale" in value &&
    typeof value.baseLocale === "string" &&
    "locales" in value &&
    Array.isArray(value.locales)
  ) {
    const locales = value.locales.filter((locale): locale is string => typeof locale === "string");
    if (locales.length === value.locales.length) {
      return { baseLocale: value.baseLocale, locales };
    }
  }
  throw new Error(`${source} does not have the expected shape`);
}

export function parseCatalog(value: unknown, source: string): Catalog {
  if (typeof value !== "object" || value === null) {
    throw new Error(`${source} is not an object`);
  }
  const entries: [string, string][] = [];
  for (const [key, message] of Object.entries(value)) {
    if (typeof message !== "string") {
      throw new Error(`${source}: ${key} is not a string`);
    }
    entries.push([key, message]);
  }
  return Object.fromEntries(entries);
}

// The compiler interpolates ICU date and time arguments raw, so a
// message must take a preformatted Intl string in a plain argument.
function hasDateOrTime(elements: MessageFormatElement[]): boolean {
  return elements.some((element) => {
    if (isDateElement(element) || isTimeElement(element)) {
      return true;
    }
    if (isPluralElement(element) || isSelectElement(element)) {
      return Object.values(element.options).some((option) => hasDateOrTime(option.value));
    }
    return isTagElement(element) && hasDateOrTime(element.children);
  });
}

function parseIssue(key: string, message: string): string | undefined {
  try {
    const elements = parse(message);
    if (hasDateOrTime(elements)) {
      return `${key}: uses an ICU date or time formatter, which never reaches Intl; pass a preformatted string instead`;
    }
    return undefined;
  } catch (error) {
    return `${key}: does not parse as ICU MessageFormat (${String(error)})`;
  }
}

function catalogEntryIssues(locale: string, catalog: Catalog): string[] {
  const issues: string[] = [];
  for (const [key, message] of Object.entries(catalog)) {
    if (message.trim() === "") {
      issues.push(`${locale}: ${key} is empty`);
      continue;
    }
    const issue = parseIssue(key, message);
    if (issue !== undefined) {
      issues.push(`${locale}: ${issue}`);
    }
  }
  return issues;
}

function keyParityIssues(
  baseLocale: string,
  base: Catalog,
  locale: string,
  other: Catalog,
): string[] {
  const issues: string[] = [];
  for (const key of Object.keys(base)) {
    if (!(key in other)) {
      issues.push(`${locale}: ${key} is missing`);
    }
  }
  for (const key of Object.keys(other)) {
    if (!(key in base)) {
      issues.push(`${locale}: ${key} does not exist in ${baseLocale}`);
    }
  }
  return issues;
}

// Every locale must carry exactly the base locale's keys, each a
// non-empty, parsable ICU message. Violations fail the build.
export function catalogIssues(
  baseLocale: string,
  catalogs: Readonly<Record<string, Catalog>>,
): string[] {
  const byLocale = new Map(Object.entries(catalogs));
  const base = byLocale.get(baseLocale);
  if (base === undefined) {
    return [`the base locale ${baseLocale} has no catalog`];
  }
  const issues: string[] = [];
  for (const [locale, catalog] of byLocale) {
    issues.push(...catalogEntryIssues(locale, catalog));
    if (locale !== baseLocale) {
      issues.push(...keyParityIssues(baseLocale, base, locale, catalog));
    }
  }
  return issues;
}

const ACCENTED = new Map([
  ...Object.entries({ a: "á", c: "ç", e: "é", i: "í", n: "ñ", o: "ó", u: "ú", y: "ý" }),
  ...Object.entries({ A: "Á", C: "Ç", E: "É", I: "Í", N: "Ñ", O: "Ó", U: "Ú", Y: "Ý" }),
]);
// Must cover every ACCENTED key; a test holds the two together.
const ACCENTABLE = /[aceinouyACEINOUY]/gu;

// One padding dot per three characters approximates translation growth.
const EXPANSION_CHARS_PER_DOT = 3;

function pseudoText(text: string): string {
  const accented = text.replace(ACCENTABLE, (char) => ACCENTED.get(char) ?? char);
  const dots = "·".repeat(Math.ceil(text.length / EXPANSION_CHARS_PER_DOT));
  return `${accented}${dots}`;
}

function pseudoLocalizeElements(elements: MessageFormatElement[]): void {
  for (const element of elements) {
    if (isLiteralElement(element)) {
      element.value = pseudoText(element.value);
    } else if (isPluralElement(element) || isSelectElement(element)) {
      for (const option of Object.values(element.options)) {
        pseudoLocalizeElements(option.value);
      }
    } else if (isTagElement(element)) {
      pseudoLocalizeElements(element.children);
    }
  }
}

// Accents and widens the literal text of an ICU message while leaving
// arguments, plural and select machinery untouched.
export function pseudoLocalize(message: string): string {
  const ast = parse(message);
  pseudoLocalizeElements(ast);
  return printAST(ast);
}

export function pseudoCatalog(base: Catalog): Catalog {
  return Object.fromEntries(
    Object.entries(base).map(([key, message]) => [key, pseudoLocalize(message)]),
  );
}
