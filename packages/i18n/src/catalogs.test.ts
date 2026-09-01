// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import { parse } from "@formatjs/icu-messageformat-parser";
import { expect, test } from "vitest";

import {
  catalogIssues,
  parseCatalog,
  parseSettings,
  pseudoCatalog,
  pseudoLocalize,
} from "./catalogs.js";

test("aligned catalogs produce no issues", () => {
  const issues = catalogIssues("en", {
    en: { greeting: "Hello {name}." },
    nl: { greeting: "Hallo {name}." },
  });
  expect(issues).toEqual([]);
});

test("a key missing from a translation is an issue", () => {
  const issues = catalogIssues("en", {
    en: { greeting: "Hello.", farewell: "Bye." },
    nl: { greeting: "Hallo." },
  });
  expect(issues).toEqual(["nl: farewell is missing"]);
});

test("a key absent from the base locale is an issue", () => {
  const issues = catalogIssues("en", {
    en: { greeting: "Hello." },
    nl: { greeting: "Hallo.", extra: "Te veel." },
  });
  expect(issues).toEqual(["nl: extra does not exist in en"]);
});

test("an ICU date or time formatter in a message is an issue", () => {
  const issues = catalogIssues("en", {
    en: { when: "It happened {when, date, full}.", at: "At {at, time, short}." },
  });
  expect(issues).toHaveLength(2);
  expect(issues[0]).toContain("when: uses an ICU date or time formatter");
  expect(issues[1]).toContain("at: uses an ICU date or time formatter");
});

test("empty and unparsable messages are issues", () => {
  const issues = catalogIssues("en", {
    en: { blank: "  ", broken: "Hello {name" },
  });
  expect(issues).toHaveLength(2);
  expect(issues[0]).toContain("blank is empty");
  expect(issues[1]).toContain("does not parse");
});

test("pseudolocalization accents and widens literal text", () => {
  const pseudo = pseudoLocalize("Hello world.");
  expect(pseudo).toContain("Hélló wórld.");
  expect(pseudo).toContain("·");
});

test("every accentable letter maps to its accented form", () => {
  expect(pseudoLocalize("aceinouy ACEINOUY")).toContain("áçéíñóúý ÁÇÉÍÑÓÚÝ");
});

test("pseudolocalization keeps ICU machinery intact", () => {
  const pseudo = pseudoLocalize("{count, plural, one {# message} other {# messages}}");
  expect(() => parse(pseudo)).not.toThrow();
  expect(pseudo).toContain("count");
  expect(pseudo).toContain("plural");
  expect(pseudo).toContain("mésságé");
});

test("pseudolocalization leaves argument names untouched", () => {
  const pseudo = pseudoLocalize("Today is {today, date, full}.");
  expect(pseudo).toContain("{today,");
  expect(pseudo).toContain("Tódáý");
});

test("a pseudo catalog covers every base key", () => {
  const pseudo = pseudoCatalog({ greeting: "Hello.", farewell: "Bye." });
  expect(Object.keys(pseudo)).toEqual(["greeting", "farewell"]);
  expect(pseudo.greeting).toContain("Hélló");
});

test("settings parse only in the expected shape", () => {
  expect(parseSettings({ baseLocale: "en", locales: ["en", "nl"] }, "settings")).toEqual({
    baseLocale: "en",
    locales: ["en", "nl"],
  });
  expect(() => parseSettings({ locales: ["en"] }, "settings")).toThrow("expected shape");
  expect(() => parseSettings({ baseLocale: "en", locales: [1] }, "settings")).toThrow(
    "expected shape",
  );
});

test("a catalog parses only string values", () => {
  expect(parseCatalog({ greeting: "Hello." }, "catalog")).toEqual({ greeting: "Hello." });
  expect(() => parseCatalog({ greeting: 1 }, "catalog")).toThrow("greeting is not a string");
  expect(() => parseCatalog(null, "catalog")).toThrow("not an object");
});
